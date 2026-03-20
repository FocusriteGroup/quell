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
    let exit_code = run_proxy(&command_line, config, tool)?;

    info!(exit_code, "quell shutting down");

    // Force-exit the process. The input thread may still be blocked in
    // ReadFile on the console handle (Windows doesn't support cancelling
    // console reads reliably). Without this, the process hangs after
    // main() returns because Rust's runtime waits for all threads.
    std::process::exit(exit_code as i32);
}

fn run_proxy(command_line: &str, config: AppConfig, tool: config::ToolKind) -> Result<u32> {
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
            // Extract the base command name for friendly messages
            let cmd_name = command_line.split_whitespace().next().unwrap_or(command_line);
            let wrapped = anyhow::Error::from(e).context("failed to create ConPTY session");
            if print_friendly_spawn_error(cmd_name, &wrapped) {
                // Friendly message already printed — log the full chain for --verbose
                // and exit cleanly without dumping the anyhow chain to stderr.
                tracing::debug!(error = %wrapped, "spawn failed");
                std::process::exit(1);
            }
            return Err(wrapped);
        }
    };

    // Create and run the proxy
    let (proxy, _events) = Proxy::new(config, tool, session);
    let result = proxy.run();

    // Always restore console mode, even if proxy.run() failed
    if let Err(e) = console_mode.restore() {
        warn!(error = %e, "failed to restore console mode");
    }
    // Prevent double-restore in Drop
    std::mem::forget(console_mode);

    result
}

/// Print a branded startup banner to stderr.
fn print_banner(command: &str) {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!(" \u{250f}\u{2501}\u{2501} quell \u{2501}\u{2501}\u{2513}");
    eprintln!(" \u{2503}  v{version:<6}\u{2503}  launching '{command}' \u{00b7} scroll-fix: active");
    eprintln!(" \u{2517}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{2501}\u{251b}");
}

/// Print a friendly error message for known Windows spawn failures.
/// Returns true if a friendly message was printed.
fn print_friendly_spawn_error(command: &str, error: &anyhow::Error) -> bool {
    // Walk the error chain looking for a windows::core::Error
    for cause in error.chain() {
        if let Some(win_err) = cause.downcast_ref::<windows::core::Error>() {
            let code = win_err.code().0 as u32;
            match code {
                // ERROR_FILE_NOT_FOUND
                0x80070002 => {
                    eprintln!("error: '{command}' not found.");
                    eprintln!("  Make sure it's installed and on your PATH.");
                    eprintln!("  Run 'where {command}' to check.");
                    return true;
                }
                // ERROR_ACCESS_DENIED
                0x80070005 => {
                    eprintln!("error: Permission denied when launching '{command}'.");
                    eprintln!("  Try running as administrator.");
                    return true;
                }
                _ => {}
            }
        }
    }
    false
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
