// Unix platform implementation — macOS PTY backend.
// Implements PtySession and TerminalMode traits using nix crate.

use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use nix::errno::Errno;
use nix::libc;
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::sys::signal::{self, Signal};
use nix::sys::termios::{self, SetArg, Termios};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::config::ToolKind;
use crate::proxy::ShutdownReason;

use super::{PtyReader, PtySession, PtyWriter, TerminalMode};

// ---------------------------------------------------------------------------
// PtyReader / PtyWriter over raw fd
// ---------------------------------------------------------------------------

/// Reader for the PTY master fd. Uses raw fd so `read` takes `&self`.
struct UnixPtyReader {
    fd: RawFd,
}

// SAFETY: The fd is owned by PlatformPtySession and valid for the reader's lifetime.
// The reader is moved to a dedicated I/O thread.
unsafe impl Send for UnixPtyReader {}

impl PtyReader for UnixPtyReader {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let n = nix::unistd::read(self.fd, buf);
        match n {
            Ok(0) => Ok(0),
            Ok(n) => Ok(n),
            Err(Errno::EIO) => Ok(0), // EIO on PTY master means child closed slave side
            Err(e) => Err(e.into()),
        }
    }
}

/// Writer for the PTY master fd. Uses raw fd so `write_all` takes `&self`.
struct UnixPtyWriter {
    fd: RawFd,
}

// SAFETY: Same rationale as UnixPtyReader.
unsafe impl Send for UnixPtyWriter {}

impl PtyWriter for UnixPtyWriter {
    fn write_all(&self, buf: &[u8]) -> Result<()> {
        let mut written = 0;
        while written < buf.len() {
            // SAFETY: fd is valid, buf slice is valid
            let n = unsafe {
                libc::write(
                    self.fd,
                    buf[written..].as_ptr() as *const libc::c_void,
                    buf.len() - written,
                )
            };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                return Err(err.into());
            }
            written += n as usize;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlatformPtySession
// ---------------------------------------------------------------------------

pub struct PlatformPtySession {
    master_fd: OwnedFd,
    child_pid: Pid,
    cols: i16,
    rows: i16,
    io_taken: bool,
}

impl PtySession for PlatformPtySession {
    fn spawn(command: &str, cols: i16, rows: i16) -> Result<Self> {
        let winsize = Winsize {
            ws_col: cols as u16,
            ws_row: rows as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // SAFETY: forkpty is unsafe because it calls fork(). The child process
        // immediately execs, which is the safe pattern.
        let result = unsafe { forkpty(&winsize, None) }
            .context("forkpty failed")?;

        match result {
            ForkptyResult::Child => {
                // In the child process — exec the command via the user's shell.
                // We use /bin/sh -c to handle command lines with arguments.
                let c_shell =
                    std::ffi::CString::new("/bin/sh").expect("CString::new failed");
                let c_flag = std::ffi::CString::new("-c").expect("CString::new failed");
                let c_cmd =
                    std::ffi::CString::new(command).expect("CString::new failed");
                let argv: [*const libc::c_char; 4] = [
                    c_shell.as_ptr(),
                    c_flag.as_ptr(),
                    c_cmd.as_ptr(),
                    std::ptr::null(),
                ];
                unsafe { libc::execvp(c_shell.as_ptr(), argv.as_ptr()) };
                // If exec failed, exit immediately (we're in the child)
                unsafe { libc::_exit(127) };
            }
            ForkptyResult::Parent { child, master } => {
                Ok(PlatformPtySession {
                    master_fd: master,
                    child_pid: child,
                    cols,
                    rows,
                    io_taken: false,
                })
            }
        }
    }

    fn take_io(&mut self) -> Option<(Box<dyn PtyWriter>, Box<dyn PtyReader>)> {
        if self.io_taken {
            return None;
        }
        self.io_taken = true;
        let fd = self.master_fd.as_raw_fd();
        let writer = Box::new(UnixPtyWriter { fd });
        let reader = Box::new(UnixPtyReader { fd });
        Some((writer, reader))
    }

    fn size(&self) -> (i16, i16) {
        (self.cols, self.rows)
    }

    fn resize(&mut self, cols: i16, rows: i16) -> Result<()> {
        let ws = Winsize {
            ws_col: cols as u16,
            ws_row: rows as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // TIOCSWINSZ ioctl to resize the PTY
        let ret = unsafe {
            libc::ioctl(self.master_fd.as_raw_fd(), libc::TIOCSWINSZ, &ws)
        };
        if ret < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }

    fn process_handle_raw(&self) -> usize {
        self.child_pid.as_raw() as usize
    }

    fn try_wait_for_child(&self, _timeout_ms: u32) -> Result<Option<u32>> {
        match waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => Ok(Some(code as u32)),
            Ok(WaitStatus::Signaled(_, sig, _)) => Ok(Some(128 + sig as u32)),
            Ok(WaitStatus::StillAlive) => Ok(None),
            Ok(_) => Ok(None), // Stopped, Continued, etc. — treat as still running
            Err(Errno::ECHILD) => Ok(Some(0)), // Already reaped
            Err(e) => Err(e.into()),
        }
    }

    fn process_id(&self) -> u32 {
        self.child_pid.as_raw() as u32
    }
}

impl Drop for PlatformPtySession {
    fn drop(&mut self) {
        // Send SIGHUP to the child, then reap it.
        let _ = signal::kill(self.child_pid, Signal::SIGHUP);
        // Non-blocking wait first
        match waitpid(self.child_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => {
                // Give it a moment, then SIGKILL
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = signal::kill(self.child_pid, Signal::SIGKILL);
                let _ = waitpid(self.child_pid, None);
            }
            Ok(_) => {} // Already exited
            Err(_) => {} // Already reaped
        }
        // master_fd is closed automatically by OwnedFd drop
    }
}

// ---------------------------------------------------------------------------
// PlatformTerminalMode
// ---------------------------------------------------------------------------

/// Saved terminal state for emergency_restore (global, set once).
static SAVED_TERMIOS: OnceLock<Mutex<Option<Termios>>> = OnceLock::new();

pub struct PlatformTerminalMode {
    original: Termios,
}

/// Get a BorrowedFd for stdin. SAFETY: stdin fd (0) is always valid during process lifetime.
fn stdin_fd() -> BorrowedFd<'static> {
    unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) }
}

impl TerminalMode for PlatformTerminalMode {
    fn save_and_set_raw() -> Result<Self> {
        let fd = stdin_fd();
        let original = termios::tcgetattr(fd)
            .context("tcgetattr failed — stdin may not be a terminal")?;

        // Store for emergency_restore
        let global = SAVED_TERMIOS.get_or_init(|| Mutex::new(None));
        *global.lock().unwrap() = Some(original.clone());

        // Make raw
        let mut raw = original.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(fd, SetArg::TCSANOW, &raw)
            .context("tcsetattr failed — could not set raw mode")?;

        Ok(PlatformTerminalMode { original })
    }

    fn restore(&self) -> Result<()> {
        let fd = stdin_fd();
        termios::tcsetattr(fd, SetArg::TCSANOW, &self.original)
            .context("tcsetattr failed — could not restore terminal")?;
        // Clear global saved state
        if let Some(global) = SAVED_TERMIOS.get() {
            *global.lock().unwrap() = None;
        }
        Ok(())
    }

    fn emergency_restore() {
        if let Some(global) = SAVED_TERMIOS.get()
            && let Ok(guard) = global.lock()
            && let Some(ref original) = *guard
        {
            let fd = stdin_fd();
            let _ = termios::tcsetattr(fd, SetArg::TCSANOW, original);
        }
    }
}

impl PlatformTerminalMode {
    #[allow(clippy::forget_non_drop)] // Intentional: documents "do not drop" semantics
    pub fn forget(self) {
        std::mem::forget(self);
    }

    #[allow(dead_code)] // API parity with Windows impl
    #[allow(clippy::forget_non_drop)]
    pub fn restore_and_forget(self) -> Result<()> {
        let result = self.restore();
        std::mem::forget(self);
        result
    }
}

// ---------------------------------------------------------------------------
// Platform functions
// ---------------------------------------------------------------------------

pub fn get_terminal_size() -> Option<(i16, i16)> {
    use std::mem::MaybeUninit;
    unsafe {
        let mut ws = MaybeUninit::<libc::winsize>::uninit();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, ws.as_mut_ptr()) == 0 {
            let ws = ws.assume_init();
            Some((ws.ws_col as i16, ws.ws_row as i16))
        } else {
            None
        }
    }
}

pub fn raw_write_stdout(data: &[u8]) -> Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(data)?;
    lock.flush()?;
    Ok(())
}

pub fn is_stdin_interactive() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

pub fn create_shutdown_signal() -> Result<usize> {
    crate::input::unix::create_shutdown_signal()
}

pub fn signal_shutdown(handle: usize) {
    crate::input::unix::signal_shutdown(handle);
}

pub fn wait_for_child_exit(handle_raw: usize, exit_status: Arc<Mutex<Option<u32>>>) {
    // handle_raw is the child PID on Unix
    let pid = Pid::from_raw(handle_raw as i32);
    loop {
        match waitpid(pid, None) {
            Ok(WaitStatus::Exited(_, code)) => {
                *exit_status.lock().unwrap() = Some(code as u32);
                break;
            }
            Ok(WaitStatus::Signaled(_, sig, _)) => {
                *exit_status.lock().unwrap() = Some(128 + sig as u32);
                break;
            }
            Ok(_) => continue, // Stopped/Continued — keep waiting
            Err(Errno::EINTR) => continue,
            Err(_) => break, // ECHILD or other — child already reaped
        }
    }
}

pub fn run_console_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
    shutdown_signal_handle: usize,
    resize_tx: Sender<(i16, i16)>,
    tool: ToolKind,
) {
    crate::input::unix::run_console_input_loop(
        input_write,
        flag,
        shutdown_tx,
        shutdown_signal_handle,
        resize_tx,
        tool,
    );
}

pub fn run_pipe_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
) {
    crate::input::unix::run_pipe_input_loop(input_write, flag, shutdown_tx);
}

/// On Unix, spawn errors don't have Windows error codes.
/// This always returns false (no friendly message printed).
#[allow(dead_code)] // Called from platform::mod via the bin target
pub fn print_friendly_spawn_error(_command: &str, _error: &anyhow::Error) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_pty_spawn() {
        let session = PlatformPtySession::spawn("/bin/echo hello", 80, 24)
            .expect("spawn should succeed");
        let mut session = session;
        let (_, reader) = session.take_io().expect("take_io should return handles");

        let mut buf = vec![0u8; 4096];
        let mut output = Vec::new();
        // Read until EOF
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("hello"),
            "expected output to contain 'hello', got: {text:?}"
        );
    }

    #[test]
    fn test_pty_read_write() {
        let mut session = PlatformPtySession::spawn("/bin/cat", 80, 24)
            .expect("spawn should succeed");
        let (writer, reader) = session.take_io().expect("take_io should return handles");

        // Write some data
        writer.write_all(b"test data\n").expect("write should succeed");

        // Read it back — cat echoes input on the PTY
        let mut buf = vec![0u8; 4096];
        let mut output = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if std::time::Instant::now() > deadline {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains("test data") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("test data"),
            "expected output to contain 'test data', got: {text:?}"
        );
    }

    #[test]
    fn test_pty_resize() {
        let mut session = PlatformPtySession::spawn("/bin/cat", 80, 24)
            .expect("spawn should succeed");
        // Resize should succeed (TIOCSWINSZ ioctl)
        session.resize(120, 40).expect("resize should succeed");
        assert_eq!(session.size(), (120, 40));
        session.resize(40, 10).expect("resize should succeed again");
        assert_eq!(session.size(), (40, 10));
    }

    #[test]
    fn test_pty_wait() {
        // Helper: spawn a command, drain PTY output in background, poll for exit
        fn wait_for_exit(command: &str) -> u32 {
            let mut session = PlatformPtySession::spawn(command, 80, 24)
                .expect("spawn should succeed");
            let (_, reader) = session.take_io().expect("take_io should return handles");

            // Drain output in a background thread so the child isn't blocked on write
            let drain = thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => continue,
                    }
                }
            });

            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let code = loop {
                if std::time::Instant::now() > deadline {
                    panic!("timed out waiting for {command}");
                }
                match session.try_wait_for_child(0) {
                    Ok(Some(code)) => break code,
                    Ok(None) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => panic!("unexpected error waiting for {command}: {e}"),
                }
            };
            let _ = drain.join();
            code
        }

        // `true` exits 0 (shell builtin; /bin/true doesn't exist on macOS)
        let code = wait_for_exit("true");
        assert_eq!(code, 0, "true should exit with code 0");

        // `false` exits 1 (shell builtin; /bin/false doesn't exist on macOS)
        let code = wait_for_exit("false");
        assert_ne!(code, 0, "false should exit with non-zero code");
    }

    #[test]
    fn test_terminal_mode_save_restore() {
        // This test can only meaningfully run when stdin is a terminal.
        // In CI/pipes, tcgetattr will fail — skip gracefully.
        let fd = stdin_fd();
        let original = match termios::tcgetattr(fd) {
            Ok(t) => t,
            Err(_) => {
                eprintln!("stdin is not a terminal — skipping terminal mode test");
                return;
            }
        };

        // Save and set raw
        let mode = PlatformTerminalMode::save_and_set_raw()
            .expect("save_and_set_raw should succeed");

        // Verify we're in raw mode (ECHO should be off)
        let current = termios::tcgetattr(fd).expect("tcgetattr should work");
        assert!(
            !current
                .local_flags
                .contains(termios::LocalFlags::ECHO),
            "ECHO should be disabled in raw mode"
        );

        // Restore
        mode.restore().expect("restore should succeed");

        // Verify original state is restored
        let restored = termios::tcgetattr(fd).expect("tcgetattr should work");
        assert_eq!(
            original.local_flags, restored.local_flags,
            "local flags should match after restore"
        );
        assert_eq!(
            original.input_flags, restored.input_flags,
            "input flags should match after restore"
        );
    }

    /// Regression test for exit code race condition: when the child-watcher
    /// thread reaps the child via blocking waitpid before the main loop reads
    /// the exit code, the stored status must reflect the real exit code (not 0).
    #[test]
    fn test_exit_code_race_condition_false() {
        let mut session = PlatformPtySession::spawn("false", 80, 24)
            .expect("spawn should succeed");
        let (_, reader) = session.take_io().expect("take_io should return handles");

        // Drain output so the child isn't blocked
        let drain = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        });

        let exit_status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let watcher_status = exit_status.clone();
        let handle_raw = session.process_handle_raw();

        // Simulate the watcher thread: blocking waitpid that stores exit status
        let watcher = thread::spawn(move || {
            wait_for_child_exit(handle_raw, watcher_status);
        });

        watcher.join().expect("watcher thread should complete");
        let _ = drain.join();

        // The watcher has reaped the child. Now the stored status must be non-zero.
        let code = exit_status.lock().unwrap()
            .expect("exit status should be stored by watcher");
        assert_eq!(code, 1, "false should produce exit code 1, got {code}");

        // Verify that try_wait_for_child now returns ECHILD -> 0 (the bug we're avoiding)
        let fallback = session.try_wait_for_child(0).unwrap();
        assert_eq!(
            fallback,
            Some(0),
            "try_wait_for_child after reap should return 0 (ECHILD), demonstrating the race"
        );
    }

    /// Verify that exit code 0 is correctly stored for a successful command.
    #[test]
    fn test_exit_code_race_condition_true() {
        let mut session = PlatformPtySession::spawn("true", 80, 24)
            .expect("spawn should succeed");
        let (_, reader) = session.take_io().expect("take_io should return handles");

        let drain = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        });

        let exit_status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let watcher_status = exit_status.clone();
        let handle_raw = session.process_handle_raw();

        let watcher = thread::spawn(move || {
            wait_for_child_exit(handle_raw, watcher_status);
        });

        watcher.join().expect("watcher thread should complete");
        let _ = drain.join();

        let code = exit_status.lock().unwrap()
            .expect("exit status should be stored by watcher");
        assert_eq!(code, 0, "true should produce exit code 0, got {code}");
    }

    #[test]
    fn test_pty_cleanup_on_drop() {
        let pid: i32;
        {
            let session = PlatformPtySession::spawn("/bin/sleep 60", 80, 24)
                .expect("spawn should succeed");
            pid = session.child_pid.as_raw();
            // Verify child is running
            let ret = unsafe { libc::kill(pid, 0) };
            assert_eq!(ret, 0, "child should be running before drop");
            // session drops here
        }
        // After drop, give a moment for cleanup
        thread::sleep(Duration::from_millis(200));
        // Verify child is gone — kill(pid, 0) should fail with ESRCH
        let ret = unsafe { libc::kill(pid, 0) };
        assert_ne!(ret, 0, "child should be reaped after drop");
    }
}
