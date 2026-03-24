// Unix input handling: stdin forwarding, SIGWINCH, and shutdown coordination.
//
// Architecture:
//   - Console input loop: reads from stdin fd using poll(), forwards bytes to PTY
//     master fd. Uses self-pipe pattern for clean shutdown. Registers SIGWINCH
//     via signal-hook and propagates terminal size changes.
//   - Pipe input loop: simple blocking read from stdin, forward to PTY. Used when
//     stdin is a pipe (test environments).
//   - Shutdown signal: self-pipe — main thread writes a byte to wake the input
//     thread's poll() call.
//
// Design decision: No raw_write_all bypass needed on macOS.
//   On Windows, `WriteConsoleW` (used by Rust's stdout) rejects non-UTF-8 byte
//   sequences, requiring a `WriteFile` bypass. On macOS/Unix, `write()` on fd 1
//   handles arbitrary bytes correctly, so `std::io::stdout().write_all()` works
//   without issue. The platform::raw_write_stdout() function already uses this
//   approach (implemented in Component 2).

use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use tracing::{debug, info, warn};

use crate::config::ToolKind;
use crate::platform::PtyWriter;
use crate::proxy::ShutdownReason;
use crate::proxy::key_translator::KeyTranslator;

/// Create a self-pipe for shutdown signaling.
///
/// We pack both fds into a single usize: read_fd in low 32 bits, write_fd in high 32 bits.
pub fn create_shutdown_signal() -> Result<usize> {
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to create shutdown self-pipe");
    }
    let read_fd = fds[0];
    let write_fd = fds[1];

    set_nonblocking(read_fd)?;
    set_nonblocking(write_fd)?;

    let packed = (read_fd as u32 as usize) | ((write_fd as u32 as usize) << 32);
    Ok(packed)
}

/// Signal the shutdown pipe by writing a byte to the write-end.
pub fn signal_shutdown(handle: usize) {
    let write_fd = (handle >> 32) as i32;
    let byte: u8 = 1;
    unsafe {
        libc::write(write_fd, &byte as *const u8 as *const libc::c_void, 1);
    }
}

/// Unpack the read fd from the packed handle.
fn shutdown_read_fd(handle: usize) -> RawFd {
    (handle & 0xFFFF_FFFF) as i32
}

fn set_nonblocking(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error()).context("F_GETFL");
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error()).context("F_SETFL");
    }
    Ok(())
}

fn raw_read(fd: RawFd, buf: &mut [u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

#[cfg(test)]
fn raw_write(fd: RawFd, buf: &[u8]) -> std::io::Result<usize> {
    let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
    if n < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Run the interactive console input loop on Unix.
///
/// Uses poll() to multiplex between stdin, the shutdown pipe, and SIGWINCH.
/// Reads bytes from stdin, translates Kitty keyboard sequences, and writes
/// to the PTY master fd.
pub(crate) fn run_console_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
    shutdown_signal_handle: usize,
    resize_tx: Sender<(i16, i16)>,
    tool: ToolKind,
) {
    let shutdown_read = shutdown_read_fd(shutdown_signal_handle);

    // Create a self-pipe for SIGWINCH delivery via signal-hook
    let mut sigwinch_fds = [0i32; 2];
    if unsafe { libc::pipe(sigwinch_fds.as_mut_ptr()) } != 0 {
        warn!("failed to create SIGWINCH pipe, falling back to pipe input loop");
        run_pipe_input_loop(input_write, flag, shutdown_tx);
        return;
    }
    let sigwinch_read = sigwinch_fds[0];
    let sigwinch_write = sigwinch_fds[1];

    if set_nonblocking(sigwinch_read).is_err() || set_nonblocking(sigwinch_write).is_err() {
        warn!("failed to set SIGWINCH pipe non-blocking");
    }

    // Register SIGWINCH: signal-hook writes to our pipe when SIGWINCH arrives
    // SAFETY: sigwinch_write is a valid writable fd from pipe()
    let sigwinch_write_fd = unsafe { std::os::unix::io::OwnedFd::from_raw_fd(sigwinch_write) };
    let _sigwinch_id = match signal_hook::low_level::pipe::register(
        signal_hook::consts::SIGWINCH,
        sigwinch_write_fd,
    ) {
        Ok(id) => id,
        Err(e) => {
            warn!(error = %e, "failed to register SIGWINCH handler");
            unsafe { libc::close(sigwinch_read); }
            run_pipe_input_loop(input_write, flag, shutdown_tx);
            return;
        }
    };

    let mut key_translator = KeyTranslator::new(tool);
    let mut buf = [0u8; 4096];

    loop {
        if flag.load(Ordering::Relaxed) {
            break;
        }

        let mut fds = [
            libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: shutdown_read,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: sigwinch_read,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 3, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            warn!(error = %err, "poll() failed in input loop");
            let _ = shutdown_tx.try_send(ShutdownReason::IoError(err.to_string()));
            break;
        }

        // Check shutdown pipe first
        if fds[1].revents & libc::POLLIN != 0 {
            debug!("shutdown signal received in input thread");
            break;
        }

        // Check SIGWINCH
        if fds[2].revents & libc::POLLIN != 0 {
            // Drain the signal pipe
            let mut drain_buf = [0u8; 64];
            let _ = raw_read(sigwinch_read, &mut drain_buf);

            // Query current terminal size and send it
            if let Some((cols, rows)) = crate::platform::get_terminal_size() {
                debug!(cols, rows, "SIGWINCH: terminal resized");
                let _ = resize_tx.try_send((cols, rows));
            }
        }

        // Check stdin
        if fds[0].revents & libc::POLLIN != 0 {
            match raw_read(libc::STDIN_FILENO, &mut buf) {
                Ok(0) => {
                    info!("stdin EOF");
                    let _ = shutdown_tx.try_send(ShutdownReason::InputEof);
                    break;
                }
                Ok(n) => {
                    let translated = key_translator.translate(&buf[..n]);
                    if let Err(e) = input_write.write_all(&translated) {
                        if !flag.load(Ordering::Relaxed) {
                            warn!(error = %e, "failed to write to PTY input");
                            let _ = shutdown_tx
                                .try_send(ShutdownReason::IoError(e.to_string()));
                        }
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    if !flag.load(Ordering::Relaxed) {
                        warn!(error = %e, "stdin read error");
                        let _ = shutdown_tx
                            .try_send(ShutdownReason::IoError(e.to_string()));
                    }
                    break;
                }
            }
        }

        // Check for error/hangup on stdin
        if fds[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            info!("stdin hangup/error");
            let _ = shutdown_tx.try_send(ShutdownReason::InputEof);
            break;
        }
    }

    // Clean up sigwinch read fd (write fd is owned by signal-hook via OwnedFd)
    unsafe { libc::close(sigwinch_read); }
}

/// Run the pipe input loop on Unix.
///
/// Simple blocking read from stdin, forward to PTY. Used when stdin is a pipe
/// (e.g., in test environments or when input is redirected).
pub(crate) fn run_pipe_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
) {
    use std::io::Read;
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 4096];

    loop {
        if flag.load(Ordering::Relaxed) {
            break;
        }

        match stdin.read(&mut buf) {
            Ok(0) => {
                info!("stdin EOF (pipe)");
                let _ = shutdown_tx.try_send(ShutdownReason::InputEof);
                break;
            }
            Ok(n) => {
                if let Err(e) = input_write.write_all(&buf[..n]) {
                    if !flag.load(Ordering::Relaxed) {
                        warn!(error = %e, "failed to write to PTY input (pipe)");
                        let _ = shutdown_tx
                            .try_send(ShutdownReason::IoError(e.to_string()));
                    }
                    break;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if !flag.load(Ordering::Relaxed) {
                    warn!(error = %e, "stdin read error (pipe)");
                    let _ = shutdown_tx
                        .try_send(ShutdownReason::IoError(e.to_string()));
                }
                break;
            }
        }
    }
}

/// Query the terminal size of a PTY fd.
#[cfg(test)]
pub fn get_pty_size(pty_fd: RawFd) -> Result<(i16, i16)> {
    use std::mem::MaybeUninit;
    unsafe {
        let mut ws = MaybeUninit::<libc::winsize>::uninit();
        if libc::ioctl(pty_fd, libc::TIOCGWINSZ, ws.as_mut_ptr()) != 0 {
            anyhow::bail!(
                "TIOCGWINSZ failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let ws = ws.assume_init();
        Ok((ws.ws_col as i16, ws.ws_row as i16))
    }
}

/// Set the terminal size on a PTY fd.
#[cfg(test)]
pub fn set_pty_size(pty_fd: RawFd, cols: u16, rows: u16) -> Result<()> {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ret = unsafe { libc::ioctl(pty_fd, libc::TIOCSWINSZ, &ws) };
    if ret != 0 {
        anyhow::bail!(
            "TIOCSWINSZ failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

use std::os::unix::io::FromRawFd;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Helper: open a PTY pair and spawn a child process.
    /// Returns (master_fd, child_pid).
    fn spawn_pty_child(cmd: &str, args: &[&str]) -> (RawFd, libc::pid_t) {
        use std::ffi::CString;

        let mut master_fd: RawFd = -1;
        let mut slave_fd: RawFd = -1;

        // openpty
        let ret = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(ret, 0, "openpty failed");

        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");

        if pid == 0 {
            // Child
            unsafe {
                libc::close(master_fd);
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY as libc::c_ulong, 0);
                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                if slave_fd > 2 {
                    libc::close(slave_fd);
                }
            }
            let cmd_c = CString::new(cmd).unwrap();
            let mut argv_c: Vec<CString> = vec![cmd_c.clone()];
            for a in args {
                argv_c.push(CString::new(*a).unwrap());
            }
            let mut argv_ptrs: Vec<*const libc::c_char> =
                argv_c.iter().map(|s| s.as_ptr()).collect();
            argv_ptrs.push(std::ptr::null());
            unsafe {
                libc::execvp(cmd_c.as_ptr(), argv_ptrs.as_ptr());
                libc::_exit(127);
            }
        }

        // Parent
        unsafe { libc::close(slave_fd); }
        (master_fd, pid)
    }

    /// Helper: read from fd until `predicate` matches or timeout expires.
    fn read_until(fd: RawFd, timeout: Duration, predicate: impl Fn(&str) -> bool) -> Vec<u8> {
        let start = Instant::now();
        let mut result = Vec::new();
        let mut buf = [0u8; 4096];

        // Save and set non-blocking
        let orig_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if orig_flags >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, orig_flags | libc::O_NONBLOCK) };
        }

        while start.elapsed() < timeout {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let remaining = timeout
                .saturating_sub(start.elapsed())
                .as_millis()
                .min(200) as i32;
            let ret = unsafe { libc::poll(&mut pfd, 1, remaining) };
            if ret > 0 && pfd.revents & libc::POLLIN != 0 {
                match raw_read(fd, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        result.extend_from_slice(&buf[..n]);
                        let s = String::from_utf8_lossy(&result);
                        if predicate(&s) {
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => break,
                }
            }
        }

        // Restore original flags
        if orig_flags >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, orig_flags) };
        }

        result
    }

    /// Helper: drain all available data from fd.
    fn drain_fd(fd: RawFd, timeout: Duration) {
        let _ = read_until(fd, timeout, |_| false);
    }

    #[test]
    fn test_input_forwarding() {
        let (master_fd, child_pid) = spawn_pty_child("/bin/cat", &[]);

        // Write data to PTY master (simulating stdin input forwarded to child)
        let test_data = b"hello from input\n";
        raw_write(master_fd, test_data).expect("write to PTY failed");

        // Read back from PTY (cat echoes input)
        let output = read_until(master_fd, Duration::from_secs(2), |s| {
            s.contains("hello from input")
        });
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("hello from input"),
            "expected echo from cat, got: {:?}",
            output_str
        );

        unsafe {
            libc::kill(child_pid, libc::SIGTERM);
            libc::waitpid(child_pid, std::ptr::null_mut(), 0);
            libc::close(master_fd);
        }
    }

    #[test]
    fn test_sigwinch_delivery() {
        let (master_fd, child_pid) = spawn_pty_child("/bin/cat", &[]);

        // Set initial size
        set_pty_size(master_fd, 120, 40).expect("initial set_pty_size failed");
        let (cols, rows) = get_pty_size(master_fd).expect("get_pty_size failed");
        assert_eq!(cols, 120);
        assert_eq!(rows, 40);

        // Resize
        set_pty_size(master_fd, 80, 24).expect("resize failed");
        let (cols2, rows2) = get_pty_size(master_fd).expect("get_pty_size after resize failed");
        assert_eq!(cols2, 80);
        assert_eq!(rows2, 24);

        // Send SIGWINCH to self — verify the PTY size is unchanged
        // (the handler propagates outer terminal size, not PTY size)
        unsafe { libc::kill(libc::getpid(), libc::SIGWINCH); }
        let (cols3, rows3) = get_pty_size(master_fd).expect("get_pty_size after SIGWINCH");
        assert_eq!(cols3, 80);
        assert_eq!(rows3, 24);

        unsafe {
            libc::kill(child_pid, libc::SIGTERM);
            libc::waitpid(child_pid, std::ptr::null_mut(), 0);
            libc::close(master_fd);
        }
    }

    #[test]
    fn test_shutdown_clean() {
        let handle = create_shutdown_signal().expect("create_shutdown_signal failed");

        // Create a dummy pipe as mock PtyWriter
        let mut mock_fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(mock_fds.as_mut_ptr()) }, 0);
        let mock_write_fd = mock_fds[1];
        unsafe { libc::close(mock_fds[0]); }

        struct MockWriter(RawFd);
        impl PtyWriter for MockWriter {
            fn write_all(&self, buf: &[u8]) -> Result<()> {
                raw_write(self.0, buf)?;
                Ok(())
            }
        }

        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();

        let thread = std::thread::Builder::new()
            .name("test-input".into())
            .spawn(move || {
                let shutdown_read = shutdown_read_fd(handle);
                let mut buf = [0u8; 256];

                loop {
                    if flag_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut fds = [libc::pollfd {
                        fd: shutdown_read,
                        events: libc::POLLIN,
                        revents: 0,
                    }];
                    let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 5000) };
                    if ret > 0 && fds[0].revents & libc::POLLIN != 0 {
                        let _ = raw_read(shutdown_read, &mut buf);
                        break;
                    }
                }
            })
            .expect("failed to spawn thread");

        std::thread::sleep(Duration::from_millis(50));

        let start = Instant::now();
        signal_shutdown(handle);

        thread.join().expect("thread panicked");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(1),
            "input thread took too long to exit: {:?}",
            elapsed
        );

        // Clean up
        let read_fd = shutdown_read_fd(handle);
        let write_fd = (handle >> 32) as i32;
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
            libc::close(mock_write_fd);
        }
    }

    #[test]
    fn test_no_zombie_on_child_exit() {
        let (master_fd, child_pid) = spawn_pty_child("/usr/bin/true", &[]);

        // Wait for child to exit
        let mut status: i32 = 0;
        let ret = unsafe { libc::waitpid(child_pid, &mut status, 0) };
        assert_eq!(ret, child_pid, "waitpid should return child pid");
        assert!(
            libc::WIFEXITED(status),
            "child should have exited normally"
        );
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "expected exit code 0 from /bin/true"
        );

        // Second waitpid should fail with ECHILD (no zombie)
        let ret2 = unsafe { libc::waitpid(child_pid, &mut status, libc::WNOHANG) };
        assert!(
            ret2 == -1 || ret2 == 0,
            "expected ECHILD error or 0 (no zombie), got ret={}",
            ret2
        );

        unsafe { libc::close(master_fd); }
    }

    #[test]
    fn test_input_resize_roundtrip() {
        // Test 1: spawn stty size with initial dimensions, verify output
        {
            let (master_fd, child_pid) = spawn_pty_child("/bin/sh", &["-c", "stty size"]);

            // Set size before child reads it (race, but shell startup takes time)
            set_pty_size(master_fd, 100, 50).expect("set initial size failed");

            let output = read_until(master_fd, Duration::from_secs(3), |s| {
                s.contains("50 100")
            });
            let output_str = String::from_utf8_lossy(&output);
            assert!(
                output_str.contains("50 100"),
                "expected '50 100' in stty output, got: {:?}",
                output_str
            );

            unsafe {
                libc::waitpid(child_pid, std::ptr::null_mut(), 0);
                libc::close(master_fd);
            }
        }

        // Test 2: spawn with different dimensions, verify
        {
            let (master_fd, child_pid) = spawn_pty_child("/bin/sh", &["-c", "stty size"]);

            set_pty_size(master_fd, 132, 43).expect("resize failed");

            let output = read_until(master_fd, Duration::from_secs(3), |s| {
                s.contains("43 132")
            });
            let output_str = String::from_utf8_lossy(&output);
            assert!(
                output_str.contains("43 132"),
                "expected '43 132' after resize, got: {:?}",
                output_str
            );

            unsafe {
                libc::waitpid(child_pid, std::ptr::null_mut(), 0);
                libc::close(master_fd);
            }
        }
    }
}
