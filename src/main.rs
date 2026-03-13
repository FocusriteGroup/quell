mod config;
mod conpty;
mod history;
mod proxy;
mod vt;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};
use clap::Parser;
use crossbeam_channel::{bounded, select, tick, Receiver, Sender};
use tracing::{debug, error, info, trace, warn};

use config::AppConfig;
use conpty::{ConsoleMode, ConPtySession};

/// Reason a thread is signaling shutdown
#[derive(Debug, Clone)]
enum ShutdownReason {
    ChildExited,
    InputEof,
    OutputEof,
    CtrlC,
    Error(String),
}

fn main() -> Result<()> {
    let cli = config::Cli::parse();

    // Initialize logging before anything else
    let _guard = init_logging(&cli)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "claude-terminal starting"
    );

    // Load configuration
    let config = AppConfig::load(&cli).context("failed to load configuration")?;
    info!(?config, "configuration loaded");

    // Determine the child command to run
    let child_command = cli.command.unwrap_or_else(|| "claude".to_string());
    let child_args: Vec<String> = cli.args.clone();

    // Build full command line
    let command_line = if child_args.is_empty() {
        child_command.clone()
    } else {
        format!("{} {}", child_command, child_args.join(" "))
    };

    info!(
        command = %command_line,
        "launching child process"
    );

    // Run the proxy — returns the child's exit code
    let exit_code = run_proxy(&command_line)?;

    info!(exit_code, "claude-terminal shutting down");

    if exit_code != 0 {
        std::process::exit(exit_code as i32);
    }

    Ok(())
}

/// Get the current terminal size from the real stdout console.
fn get_terminal_size() -> Option<(i16, i16)> {
    conpty::get_terminal_size()
}

fn run_proxy(command_line: &str) -> Result<u32> {
    // Detect terminal size
    let (cols, rows) = get_terminal_size().unwrap_or((120, 30));
    info!(cols, rows, "detected terminal size");

    // Save console mode and set raw/VT mode
    let console_mode = ConsoleMode::save_and_set_raw()
        .context("failed to save/set console mode")?;

    // Spawn child in ConPTY
    let mut session = ConPtySession::spawn(command_line, cols, rows)
        .context("failed to create ConPTY session")?;

    // Take I/O handles for the threads
    let (input_write, output_read) = session
        .take_io()
        .context("failed to take I/O handles from session")?;

    // Channels
    let (output_tx, output_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = bounded(64);
    let (shutdown_tx, shutdown_rx): (Sender<ShutdownReason>, Receiver<ShutdownReason>) = bounded(4);

    let shutdown_flag = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    {
        let shutdown_tx = shutdown_tx.clone();
        let flag = shutdown_flag.clone();
        ctrlc_handler(shutdown_tx, flag);
    }

    // Output thread: reads from ConPTY output pipe, sends to main thread
    let output_shutdown_tx = shutdown_tx.clone();
    let output_flag = shutdown_flag.clone();
    let output_thread = thread::Builder::new()
        .name("conpty-output".into())
        .spawn(move || {
            let mut buf = vec![0u8; 8192];
            loop {
                if output_flag.load(Ordering::Relaxed) {
                    break;
                }
                match output_read.read(&mut buf) {
                    Ok(0) => {
                        info!("output pipe EOF");
                        let _ = output_shutdown_tx.try_send(ShutdownReason::OutputEof);
                        break;
                    }
                    Ok(n) => {
                        debug!(bytes = n, "output chunk received");
                        if output_tx.send(buf[..n as usize].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        if !output_flag.load(Ordering::Relaxed) {
                            warn!(error = %e, "output pipe read error");
                            let _ = output_shutdown_tx
                                .try_send(ShutdownReason::Error(e.to_string()));
                        }
                        break;
                    }
                }
            }
        })
        .context("failed to spawn output thread")?;

    // Input thread: reads from real stdin, writes to ConPTY input pipe
    let input_shutdown_tx = shutdown_tx.clone();
    let input_flag = shutdown_flag.clone();
    let _input_thread = thread::Builder::new()
        .name("conpty-input".into())
        .spawn(move || {
            use std::io::Read;

            let stdin = std::io::stdin();
            let mut stdin = stdin.lock();
            let mut buf = vec![0u8; 1024];

            loop {
                if input_flag.load(Ordering::Relaxed) {
                    break;
                }
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        info!("stdin EOF");
                        let _ = input_shutdown_tx.try_send(ShutdownReason::InputEof);
                        break;
                    }
                    Ok(n) => {
                        debug!(bytes = n, "stdin read");
                        if let Err(e) = input_write.write_all(&buf[..n]) {
                            if !input_flag.load(Ordering::Relaxed) {
                                warn!(error = %e, "input pipe write error");
                                let _ = input_shutdown_tx
                                    .try_send(ShutdownReason::Error(e.to_string()));
                            }
                            break;
                        }
                    }
                    Err(e) => {
                        if !input_flag.load(Ordering::Relaxed) {
                            warn!(error = %e, "stdin read error");
                            let _ = input_shutdown_tx
                                .try_send(ShutdownReason::Error(e.to_string()));
                        }
                        break;
                    }
                }
            }
        })
        .context("failed to spawn input thread")?;

    // Note: child exit is detected by the output thread receiving EOF on the pipe.
    // No separate child-wait thread needed for raw passthrough mode.

    // Resize polling ticker
    let resize_tick = tick(std::time::Duration::from_millis(100));

    let mut stdout = std::io::stdout().lock();
    let mut last_size = (cols, rows);

    // Main loop
    info!("entering main proxy loop");
    loop {
        select! {
            recv(output_rx) -> msg => {
                match msg {
                    Ok(data) => {
                        // Raw passthrough: write child output directly to real stdout
                        if let Err(e) = stdout.write_all(&data) {
                            error!(error = %e, "failed to write to stdout");
                            break;
                        }
                        if let Err(e) = stdout.flush() {
                            error!(error = %e, "failed to flush stdout");
                            break;
                        }
                    }
                    Err(_) => {
                        info!("output channel closed");
                        break;
                    }
                }
            }
            recv(shutdown_rx) -> msg => {
                match msg {
                    Ok(reason) => {
                        info!(?reason, "shutdown signal received");
                        break;
                    }
                    Err(_) => {
                        info!("shutdown channel closed");
                        break;
                    }
                }
            }
            recv(resize_tick) -> _ => {
                if let Some((new_cols, new_rows)) = get_terminal_size() {
                    if (new_cols, new_rows) != last_size {
                        info!(
                            old_cols = last_size.0,
                            old_rows = last_size.1,
                            new_cols,
                            new_rows,
                            "terminal resize detected"
                        );
                        if let Err(e) = session.resize(new_cols, new_rows) {
                            warn!(error = %e, "failed to resize ConPTY");
                        }
                        last_size = (new_cols, new_rows);
                    } else {
                        trace!("resize poll: size unchanged");
                    }
                }
            }
        }
    }

    // Signal all threads to stop
    info!("shutting down I/O threads");
    shutdown_flag.store(true, Ordering::Relaxed);

    // Drop session to close ConPTY — this unblocks the output thread's ReadFile
    drop(session);

    // Join threads (with timeouts via drop — they'll exit when pipes break)
    let _ = output_thread.join();
    // Input thread may be blocked on stdin read — it'll unblock when process exits
    // We don't join it to avoid hanging

    // Restore console mode (explicit restore before Drop for better error reporting)
    if let Err(e) = console_mode.restore() {
        warn!(error = %e, "failed to restore console mode");
    }
    // Prevent double-restore in Drop
    std::mem::forget(console_mode);

    info!("proxy shutdown complete");

    // We don't have the exit code from the child here since we relied on output EOF.
    // Return 0 for now — the child exit code will be captured properly in a future iteration.
    Ok(0)
}

fn ctrlc_handler(shutdown_tx: Sender<ShutdownReason>, flag: Arc<AtomicBool>) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Console::SetConsoleCtrlHandler;

    struct CtrlState {
        tx: Sender<ShutdownReason>,
        flag: Arc<AtomicBool>,
    }
    // SAFETY: OnceLock ensures single initialization. The handler only reads.
    unsafe impl Sync for CtrlState {}

    static STATE: OnceLock<CtrlState> = OnceLock::new();
    STATE.get_or_init(|| CtrlState {
        tx: shutdown_tx,
        flag,
    });

    unsafe extern "system" fn handler(ctrl_type: u32) -> BOOL {
        // CTRL_C_EVENT = 0
        if ctrl_type == 0 {
            if let Some(state) = STATE.get() {
                state.flag.store(true, Ordering::Relaxed);
                let _ = state.tx.try_send(ShutdownReason::CtrlC);
            }
            return BOOL(1);
        }
        BOOL(0)
    }

    unsafe {
        let _ = SetConsoleCtrlHandler(Some(Some(handler)), true);
    }
}

fn init_logging(cli: &config::Cli) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    if let Some(log_file) = &cli.log_file {
        // File logging with structured JSON output
        let log_dir = std::path::Path::new(log_file)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let log_filename = std::path::Path::new(log_file)
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("claude-terminal.log"));

        let file_appender = tracing_appender::rolling::daily(log_dir, log_filename);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                fmt::layer()
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_line_number(true),
            )
            .init();

        Ok(Some(guard))
    } else {
        // Stderr logging for development
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(true),
            )
            .init();

        Ok(None)
    }
}
