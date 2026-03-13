mod config;
mod conpty;
mod history;
mod proxy;
mod vt;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};

use config::AppConfig;
use conpty::{ConsoleMode, ConPtySession};
use proxy::Proxy;

fn main() -> Result<()> {
    let cli = config::Cli::parse();

    // Initialize logging before anything else
    let _guard = init_logging(&cli)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "terminal-exploration starting"
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
    let exit_code = run_proxy(&command_line, config)?;

    info!(exit_code, "terminal-exploration shutting down");

    // Force-exit the process. The input thread may still be blocked in
    // ReadFile on the console handle (Windows doesn't support cancelling
    // console reads reliably). Without this, the process hangs after
    // main() returns because Rust's runtime waits for all threads.
    std::process::exit(exit_code as i32);
}

fn run_proxy(command_line: &str, config: AppConfig) -> Result<u32> {
    // Detect terminal size
    let (cols, rows) = conpty::get_terminal_size().unwrap_or((120, 30));
    info!(cols, rows, "detected terminal size");

    // Save console mode and set raw/VT mode
    let console_mode = ConsoleMode::save_and_set_raw()
        .context("failed to save/set console mode")?;

    // Install panic hook to restore console before printing panic
    install_panic_hook();

    // Spawn child in ConPTY
    let session = match ConPtySession::spawn(command_line, cols, rows) {
        Ok(s) => s,
        Err(e) => {
            // Restore console mode before propagating spawn error
            let _ = console_mode.restore();
            std::mem::forget(console_mode);
            return Err(e).context("failed to create ConPTY session");
        }
    };

    // Create and run the proxy
    let (proxy, _events) = Proxy::new(config, session);
    let result = proxy.run();

    // Always restore console mode, even if proxy.run() failed
    if let Err(e) = console_mode.restore() {
        warn!(error = %e, "failed to restore console mode");
    }
    // Prevent double-restore in Drop
    std::mem::forget(console_mode);

    result
}

/// Install a panic hook that restores console mode before printing the panic.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore console mode so the panic message is readable
        ConsoleMode::emergency_restore();
        default_hook(info);
    }));
}

fn init_logging(cli: &config::Cli) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&cli.log_level));

    if let Some(log_file) = &cli.log_file {
        // File logging with structured output — use the requested log level
        let log_dir = std::path::Path::new(log_file)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let log_filename = std::path::Path::new(log_file)
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("terminal-exploration.log"));

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
        // Stderr logging — default to warn level to avoid spamming the
        // user's terminal. Use RUST_LOG or --log-level to override.
        let stderr_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| {
                // Only override to "warn" if the user didn't explicitly set a level
                if cli.log_level == "info" {
                    EnvFilter::new("warn")
                } else {
                    EnvFilter::new(&cli.log_level)
                }
            });

        tracing_subscriber::registry()
            .with(stderr_filter)
            .with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(true),
            )
            .init();

        Ok(None)
    }
}
