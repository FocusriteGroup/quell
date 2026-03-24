mod config;
#[cfg(target_os = "windows")]
mod conpty;
mod history;
#[cfg(unix)]
mod input;
mod platform;
mod proxy;
mod vt;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};

use config::AppConfig;
use platform::{PtySession, TerminalMode, PlatformPtySession, PlatformTerminalMode};
use proxy::Proxy;

fn main() -> Result<()> {
    let cli = config::Cli::parse();

    // Initialize logging before anything else
    let _guard = init_logging(&cli)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "quell starting"
    );

    // Warn if trace-level logging is enabled — it may capture raw terminal content
    if tracing::enabled!(tracing::Level::TRACE) {
        warn!("TRACE logging is enabled — raw terminal content may appear in logs. \
               This should only be used for debugging, never in production.");
    }

    // Load configuration
    let config = AppConfig::load(&cli).context("failed to load configuration")?;
    info!(?config, "configuration loaded");

    // Determine the child command to run
    let child_command = cli.command.clone().unwrap_or_else(|| "claude".to_string());
    let child_args: Vec<String> = cli.args.clone();

    // Build full command line
    let command_line = if child_args.is_empty() {
        child_command.clone()
    } else {
        format!("{} {}", child_command, child_args.join(" "))
    };

    // Detect AI tool: CLI flag overrides auto-detection from command
    let tool = cli.tool.unwrap_or_else(|| config::ToolKind::detect(&command_line));
    info!(
        command = %command_line,
        tool = %tool,
        "launching child process"
    );

    // Print startup banner to stderr so it's always visible
    print_banner(&child_command);

    // Run the proxy — returns the child's exit code
    #[cfg(feature = "recording")]
    let exit_code = run_proxy(&command_line, config, tool, cli.record.as_deref())?;
    #[cfg(not(feature = "recording"))]
    let exit_code = run_proxy(&command_line, config, tool)?;

    info!(exit_code, "quell shutting down");

    // Force-exit the process. The input thread may still be blocked in a
    // read (Windows console reads can't be cancelled reliably, and on Unix
    // a blocking stdin read may not have been interrupted yet). Without
    // this, the process hangs after main() returns because Rust's runtime
    // waits for all threads.
    std::process::exit(exit_code as i32);
}

#[cfg(feature = "recording")]
fn run_proxy(command_line: &str, config: AppConfig, tool: config::ToolKind, record_path: Option<&str>) -> Result<u32> {
    let (cols, rows, terminal_mode, session) = setup_proxy(command_line)?;

    // Create and run the proxy
    let (proxy, _events) = Proxy::new(config, tool, session);
    let proxy = if let Some(path) = record_path {
        let recorder = proxy::recorder::VtcapRecorder::create(
            std::path::Path::new(path),
            cols as u16,
            rows as u16,
            &"quell",
        )?;
        proxy.with_recorder(recorder)
    } else {
        proxy
    };
    let result = proxy.run();

    teardown_proxy(terminal_mode);
    result
}

#[cfg(not(feature = "recording"))]
fn run_proxy(command_line: &str, config: AppConfig, tool: config::ToolKind) -> Result<u32> {
    let (_cols, _rows, terminal_mode, session) = setup_proxy(command_line)?;

    // Create and run the proxy
    let (proxy, _events) = Proxy::new(config, tool, session);
    let result = proxy.run();

    teardown_proxy(terminal_mode);
    result
}

/// Common proxy setup: detect size, set terminal mode, spawn child.
fn setup_proxy(command_line: &str) -> Result<(i16, i16, PlatformTerminalMode, PlatformPtySession)> {
    let (cols, rows) = platform::get_terminal_size().unwrap_or((120, 30));
    info!(cols, rows, "detected terminal size");

    let terminal_mode = PlatformTerminalMode::save_and_set_raw()
        .context("failed to save/set terminal mode")?;

    install_panic_hook();

    let session = match PlatformPtySession::spawn(command_line, cols, rows) {
        Ok(s) => s,
        Err(e) => {
            let _ = terminal_mode.restore();
            terminal_mode.forget();
            // Extract the base command name for friendly messages
            let cmd_name = command_line.split_whitespace().next().unwrap_or(command_line);
            let wrapped = e.context("failed to create PTY session");
            if print_friendly_spawn_error(cmd_name, &wrapped) {
                // Friendly message already printed — log the full chain for --verbose
                // and exit cleanly without dumping the anyhow chain to stderr.
                tracing::debug!(error = %wrapped, "spawn failed");
                std::process::exit(1);
            }
            return Err(wrapped);
        }
    };

    Ok((cols, rows, terminal_mode, session))
}

/// Common proxy teardown: restore terminal mode.
fn teardown_proxy(terminal_mode: PlatformTerminalMode) {
    if let Err(e) = terminal_mode.restore() {
        warn!(error = %e, "failed to restore terminal mode");
    }
    terminal_mode.forget();
}

/// Print a branded startup banner to stderr.
fn print_banner(command: &str) {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!(" \u{250f}\u{2501}\u{2501} quell \u{2501}\u{2501}\u{2513}");
    eprintln!(" \u{2503}  v{version:<6}\u{2503}  launching '{command}' \u{00b7} scroll-fix: active");
    eprintln!(" \u{2517}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{251b}");
}

/// Print a friendly error message for known spawn failures.
/// Returns true if a friendly message was printed.
fn print_friendly_spawn_error(command: &str, error: &anyhow::Error) -> bool {
    platform::print_friendly_spawn_error(command, error)
}

/// Install a panic hook that restores terminal mode before printing the panic.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore terminal mode so the panic message is readable
        PlatformTerminalMode::emergency_restore();
        default_hook(info);
    }));
}

fn init_logging(cli: &config::Cli) -> Result<Option<tracing_appender::non_blocking::WorkerGuard>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let log_level = cli.log_level.as_deref().unwrap_or("info");

    // --verbose without --log-file: auto-create a log file so debug output
    // doesn't corrupt the proxied terminal display.
    let effective_log_file = if cli.verbose && cli.log_file.is_none() {
        let path = std::env::temp_dir().join("quell-verbose.log");
        eprintln!("  verbose: logging to {}", path.display());
        Some(path.to_string_lossy().into_owned())
    } else {
        cli.log_file.clone()
    };

    let effective_level = if cli.verbose { "debug" } else { log_level };

    if let Some(log_file) = &effective_log_file {
        // File logging with structured output
        let log_dir = std::path::Path::new(log_file)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let log_filename = std::path::Path::new(log_file)
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("quell.log"));

        let file_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(effective_level));

        let file_appender = tracing_appender::rolling::daily(log_dir, log_filename);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::registry()
            .with(file_filter)
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
                if cli.log_level.is_none() {
                    EnvFilter::new("warn")
                } else {
                    EnvFilter::new(log_level)
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
