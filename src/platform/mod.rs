// Platform abstraction layer
//
// Defines traits for PTY session management and terminal mode control.
// Platform-specific implementations live behind #[cfg] gates:
//   - Windows: wraps existing ConPTY code
//   - Unix: stubs for now (Component 3 fills these in)

#[cfg(target_os = "windows")]
mod windows_impl;
#[cfg(unix)]
mod unix_impl;

use anyhow::Result;

/// A platform-specific PTY session that owns a child process.
///
/// Implementations: `ConPtySession` (Windows), `UnixPtySession` (Unix, stub).
pub trait PtySession: Send {
    /// Spawn a child process in a new PTY.
    ///
    /// `command` is the full command line string.
    /// `cols` and `rows` set the initial terminal dimensions.
    fn spawn(command: &str, cols: i16, rows: i16) -> Result<Self>
    where
        Self: Sized;

    /// Take ownership of the I/O handles for use in I/O threads.
    /// Returns (writer, reader) — writer sends to child stdin, reader gets child stdout.
    /// Can only be called once.
    fn take_io(&mut self) -> Option<(Box<dyn PtyWriter>, Box<dyn PtyReader>)>;

    /// Get current terminal dimensions as (cols, rows).
    fn size(&self) -> (i16, i16);

    /// Resize the PTY terminal.
    fn resize(&mut self, cols: i16, rows: i16) -> Result<()>;

    /// Get a raw handle value for the child process (used for watcher threads).
    /// On Windows this is the process HANDLE as usize. On Unix this is the child PID as usize.
    fn process_handle_raw(&self) -> usize;

    /// Try to wait for the child to exit with a timeout in milliseconds.
    /// Returns Ok(Some(exit_code)) if the child exited, Ok(None) on timeout.
    fn try_wait_for_child(&self, timeout_ms: u32) -> Result<Option<u32>>;

    /// Get the child process ID.
    #[cfg_attr(unix, allow(dead_code))]
    fn process_id(&self) -> u32;
}

/// Reader half of a PTY — reads child output.
pub trait PtyReader: Send {
    /// Read bytes into the buffer. Returns the number of bytes read.
    /// Returns Ok(0) on EOF.
    fn read(&self, buf: &mut [u8]) -> Result<usize>;
}

/// Writer half of a PTY — writes to child input.
pub trait PtyWriter: Send {
    /// Write all bytes to the child's stdin.
    fn write_all(&self, buf: &[u8]) -> Result<()>;
}

/// Platform-specific terminal mode management.
///
/// Saves and restores the host terminal's mode (raw mode for proxy operation).
pub trait TerminalMode: Send {
    /// Save the current terminal state and set raw mode for proxy operation.
    fn save_and_set_raw() -> Result<Self>
    where
        Self: Sized;

    /// Restore the original terminal state.
    fn restore(&self) -> Result<()>;

    /// Emergency restore from a panic hook or signal handler.
    /// Safe to call from any thread. Silently does nothing if state was never saved.
    fn emergency_restore();
}

/// Detect the current terminal size.
/// Returns (cols, rows) or None if not attached to a terminal.
pub(crate) fn get_terminal_size() -> Option<(i16, i16)> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::get_terminal_size()
    }
    #[cfg(unix)]
    {
        unix_impl::get_terminal_size()
    }
}

/// Write all bytes directly to stdout, bypassing Rust's stdout lock.
/// On Windows this uses WriteFile on the raw handle (avoids WriteConsoleW UTF-8 issues).
/// On Unix this uses std::io::stdout().write_all() (no such issue exists).
pub(crate) fn raw_write_stdout(data: &[u8]) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::raw_write_stdout(data)
    }
    #[cfg(unix)]
    {
        unix_impl::raw_write_stdout(data)
    }
}

/// Check if stdin is an interactive terminal (vs a pipe in test environments).
pub(crate) fn is_stdin_interactive() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_impl::is_stdin_interactive()
    }
    #[cfg(unix)]
    {
        unix_impl::is_stdin_interactive()
    }
}

/// Create a shutdown signaling mechanism for the input thread.
/// On Windows: returns a manual-reset Event handle.
/// On Unix: returns a self-pipe fd (stub for now).
pub(crate) fn create_shutdown_signal() -> Result<usize> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::create_shutdown_signal()
    }
    #[cfg(unix)]
    {
        unix_impl::create_shutdown_signal()
    }
}

/// Signal the shutdown mechanism to wake up the input thread.
pub(crate) fn signal_shutdown(handle: usize) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::signal_shutdown(handle);
    }
    #[cfg(unix)]
    {
        unix_impl::signal_shutdown(handle);
    }
}

/// Wait for a child process to exit (blocking).
/// `handle_raw` is the value from `PtySession::process_handle_raw()`.
/// Stores the exit status in `exit_status` before returning.
pub(crate) fn wait_for_child_exit(handle_raw: usize, exit_status: std::sync::Arc<std::sync::Mutex<Option<u32>>>) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::wait_for_child_exit(handle_raw, exit_status);
    }
    #[cfg(unix)]
    {
        unix_impl::wait_for_child_exit(handle_raw, exit_status);
    }
}

/// Run the interactive console input loop (when stdin is a terminal).
/// Reads keyboard events and resize events, writes translated bytes to the PTY.
///
/// On Windows: uses ReadConsoleInputW + WaitForMultipleObjects.
/// On Unix: stub (Component 4 fills this in).
pub(crate) fn run_console_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown_tx: crossbeam_channel::Sender<super::proxy::ShutdownReason>,
    shutdown_signal_handle: usize,
    resize_tx: crossbeam_channel::Sender<(i16, i16)>,
    tool: crate::config::ToolKind,
) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::run_console_input_loop(
            input_write,
            flag,
            shutdown_tx,
            shutdown_signal_handle,
            resize_tx,
            tool,
        );
    }
    #[cfg(unix)]
    {
        unix_impl::run_console_input_loop(
            input_write,
            flag,
            shutdown_tx,
            shutdown_signal_handle,
            resize_tx,
            tool,
        );
    }
}

/// Run the pipe input loop (when stdin is a pipe, e.g. in tests).
/// Simple blocking read from stdin, forward to PTY.
pub(crate) fn run_pipe_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown_tx: crossbeam_channel::Sender<super::proxy::ShutdownReason>,
) {
    #[cfg(target_os = "windows")]
    {
        windows_impl::run_pipe_input_loop(input_write, flag, shutdown_tx);
    }
    #[cfg(unix)]
    {
        unix_impl::run_pipe_input_loop(input_write, flag, shutdown_tx);
    }
}

/// Print a friendly error message for known platform-specific spawn failures.
/// Returns true if a friendly message was printed.
#[allow(dead_code)] // Called from the bin target
pub(crate) fn print_friendly_spawn_error(command: &str, error: &anyhow::Error) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_impl::print_friendly_spawn_error(command, error)
    }
    #[cfg(unix)]
    {
        unix_impl::print_friendly_spawn_error(command, error)
    }
}

// Re-export the concrete platform types for use in main.rs and proxy/mod.rs
#[cfg(target_os = "windows")]
pub use windows_impl::PlatformPtySession;
#[cfg(unix)]
pub use unix_impl::PlatformPtySession;

#[cfg(target_os = "windows")]
pub use windows_impl::PlatformTerminalMode;
#[cfg(unix)]
pub use unix_impl::PlatformTerminalMode;
