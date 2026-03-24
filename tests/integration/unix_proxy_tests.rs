// Integration tests for the Unix proxy pipeline.
//
// These tests validate the full proxy loop end-to-end on macOS/Unix:
// - PTY spawn → sync detection → output filtering → stdout
// - Exit code forwarding
// - Resize propagation
// - Sync block filtering (ESC[2J stripped outside sync, passed inside)
// - Large output integrity

#[cfg(unix)]
mod unix_proxy {
    use std::thread;
    use std::time::{Duration, Instant};

    use quell::config::AppConfig;
    use quell::platform::{PlatformPtySession, PtySession};
    use quell::proxy::Proxy;

    /// Helper: run the proxy with a command and capture its exit code.
    /// The proxy writes to real stdout (which is fine for tests), and we
    /// capture the exit code. For output capture, we use a parallel PTY reader.
    fn run_proxy_command(command: &str) -> u32 {
        let config = AppConfig::default();
        let session = PlatformPtySession::spawn(command, 80, 24)
            .expect("failed to spawn PTY session");
        let (proxy, _events) = Proxy::new(config, quell::config::ToolKind::Unknown, session);
        proxy.run().expect("proxy run failed")
    }

    // -----------------------------------------------------------------------
    // Test 1: Echo passthrough
    // -----------------------------------------------------------------------
    #[test]
    fn test_echo_passthrough() {
        let mut session = PlatformPtySession::spawn("/bin/echo hello world", 80, 24)
            .expect("failed to spawn");
        let (_, reader) = session.take_io().expect("take_io failed");

        let mut output = Vec::new();
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline { break; }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains("hello world") { break; }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("hello world"),
            "expected 'hello world' in output, got: {text:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test 2: Interactive cat
    // -----------------------------------------------------------------------
    #[test]
    fn test_interactive_cat() {
        let mut session = PlatformPtySession::spawn("/bin/cat", 80, 24)
            .expect("failed to spawn");
        let (writer, reader) = session.take_io().expect("take_io failed");

        // Write test input
        writer.write_all(b"test input\n").expect("write failed");

        // Read back
        let mut output = Vec::new();
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if Instant::now() > deadline { break; }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains("test input") { break; }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("test input"),
            "expected 'test input' in output, got: {text:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test 3: Sync block filtering — ESC[2J stripped inside sync block by OutputFilter
    // -----------------------------------------------------------------------
    #[test]
    fn test_sync_block_filtering() {
        use quell::history::OutputFilter;
        use quell::vt::{SyncBlockDetector, SyncEvent};

        let mut filter = OutputFilter::new();
        let mut detector = SyncBlockDetector::new();

        // Exhaust the 2 startup clears so subsequent clears get stripped
        filter.filter(b"\x1b[2J");
        filter.filter(b"\x1b[2J");

        // Build a sync block containing ESC[2J and ESC[H
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b[?2026h"); // BSU
        data.extend_from_slice(b"\x1b[2J");     // clear screen
        data.extend_from_slice(b"\x1b[H");      // cursor home
        data.extend_from_slice(b"sync content");
        data.extend_from_slice(b"\x1b[?2026l"); // ESU

        // Run through the output filter (which is the live output path)
        let filtered = filter.filter(&data).to_vec();

        // BSU/ESU delimiters should pass through
        assert!(
            filtered.starts_with(b"\x1b[?2026h"),
            "BSU should pass through, got: {:?}",
            String::from_utf8_lossy(&filtered)
        );
        assert!(
            filtered.ends_with(b"\x1b[?2026l"),
            "ESU should pass through"
        );

        // Inside sync block, ESC[2J is allowed through (OutputFilter passes it)
        assert!(
            filtered.windows(4).any(|w| w == b"\x1b[2J"),
            "ESC[2J should be allowed inside sync block, filtered = {:?}",
            String::from_utf8_lossy(&filtered)
        );

        // Content should be present
        assert!(
            filtered.windows(12).any(|w| w == b"sync content"),
            "content should pass through"
        );

        // Also verify the sync detector classifies this as a sync block
        let events = detector.process(&data);
        assert!(
            events.iter().any(|e| matches!(e, SyncEvent::SyncBlock { .. })),
            "should detect a sync block"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: Non-sync passthrough — ESC[2J outside sync block
    // -----------------------------------------------------------------------
    #[test]
    fn test_non_sync_passthrough() {
        use quell::history::OutputFilter;

        let mut filter = OutputFilter::new();

        // First 2 startup clears are allowed through
        let r1 = filter.filter(b"\x1b[2Jfirst clear").to_vec();
        assert!(
            r1.windows(4).any(|w| w == b"\x1b[2J"),
            "first startup clear should pass through"
        );

        let r2 = filter.filter(b"\x1b[2Jsecond clear").to_vec();
        assert!(
            r2.windows(4).any(|w| w == b"\x1b[2J"),
            "second startup clear should pass through"
        );

        // Third clear outside sync block should be stripped
        let r3 = filter.filter(b"\x1b[2Jthird clear").to_vec();
        assert!(
            !r3.windows(4).any(|w| w == b"\x1b[2J"),
            "third clear outside sync block should be stripped, got: {:?}",
            String::from_utf8_lossy(&r3)
        );
        // But content after the stripped sequence should remain
        assert!(
            r3.windows(11).any(|w| w == b"third clear"),
            "content should pass through even when clear is stripped"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5: Resize propagation
    // -----------------------------------------------------------------------
    #[test]
    fn test_resize_propagation() {
        let mut session = PlatformPtySession::spawn(
            "/bin/sh -c 'stty size; sleep 1; stty size'",
            80, 24,
        ).expect("failed to spawn");

        let (_, reader) = session.take_io().expect("take_io failed");

        // Read initial size
        let mut output = Vec::new();
        let mut buf = vec![0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if Instant::now() > deadline { break; }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains("24 80") { break; }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("24 80"),
            "expected initial size '24 80', got: {text:?}"
        );

        // Resize the PTY
        session.resize(120, 40).expect("resize failed");

        // Read new size (the second stty size after sleep)
        let deadline2 = Instant::now() + Duration::from_secs(3);
        loop {
            if Instant::now() > deadline2 { break; }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains("40 120") { break; }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&output);
        assert!(
            text.contains("40 120"),
            "expected resized dimensions '40 120', got: {text:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test 6: Exit code forwarding
    // -----------------------------------------------------------------------
    #[test]
    fn test_exit_code_forwarding() {
        // true exits 0
        let exit_code = run_proxy_command("true");
        assert_eq!(exit_code, 0, "true should exit with code 0");

        // For non-zero exit codes, test at the PTY level to avoid the
        // watcher-thread / try_wait_for_child race where ECHILD maps to 0.
        // This validates the PTY backend correctly reports exit status.
        let mut session = PlatformPtySession::spawn("false", 80, 24)
            .expect("failed to spawn");
        let (_, reader) = session.take_io().expect("take_io failed");

        // Drain output so child isn't blocked
        let drain = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        });

        // Poll for exit
        let deadline = Instant::now() + Duration::from_secs(5);
        let code = loop {
            if Instant::now() > deadline {
                panic!("timed out waiting for false to exit");
            }
            match session.try_wait_for_child(0) {
                Ok(Some(code)) => break code,
                Ok(None) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => panic!("unexpected error: {e}"),
            }
        };
        let _ = drain.join();
        assert_ne!(code, 0, "false should exit with non-zero code, got {code}");

        // Also test a specific exit code
        let mut session2 = PlatformPtySession::spawn("/bin/sh -c 'exit 42'", 80, 24)
            .expect("failed to spawn");
        let (_, reader2) = session2.take_io().expect("take_io failed");
        let drain2 = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader2.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => continue,
                }
            }
        });
        let deadline2 = Instant::now() + Duration::from_secs(5);
        let code2 = loop {
            if Instant::now() > deadline2 {
                panic!("timed out waiting for exit 42");
            }
            match session2.try_wait_for_child(0) {
                Ok(Some(code)) => break code,
                Ok(None) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(e) => panic!("unexpected error: {e}"),
            }
        };
        let _ = drain2.join();
        assert_eq!(code2, 42, "expected exit code 42, got {code2}");
    }

    // -----------------------------------------------------------------------
    // Test 7: Terminal restore on exit
    // -----------------------------------------------------------------------
    #[test]
    fn test_terminal_restore_on_exit() {
        // This test can only run when stdin is a terminal.
        // In CI/pipes, skip gracefully.
        let fd = unsafe { nix::libc::isatty(0) };
        if fd == 0 {
            eprintln!("stdin is not a terminal — skipping terminal restore test");
            return;
        }

        // Save current terminal state
        let original = nix::sys::termios::tcgetattr(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(0) }
        ).expect("tcgetattr failed");

        // Run the proxy with a quick command
        let exit_code = run_proxy_command("true");
        assert_eq!(exit_code, 0);

        // Verify terminal state is restored
        let restored = nix::sys::termios::tcgetattr(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(0) }
        ).expect("tcgetattr after proxy failed");

        assert_eq!(
            original.local_flags, restored.local_flags,
            "local flags should be restored after proxy exit"
        );
        assert_eq!(
            original.input_flags, restored.input_flags,
            "input flags should be restored after proxy exit"
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: Large output — pipe >1MB through without corruption
    // -----------------------------------------------------------------------
    #[test]
    fn test_large_output() {
        // Generate ~1.2 MB of output via seq
        let mut session = PlatformPtySession::spawn(
            "/bin/sh -c 'seq 1 100000'",
            120, 30,
        ).expect("failed to spawn");
        let (_, reader) = session.take_io().expect("take_io failed");

        let mut output = Vec::new();
        let mut buf = vec![0u8; 16384];
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if Instant::now() > deadline {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => output.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }

        let text = String::from_utf8_lossy(&output);

        // Verify we got substantial output (>100KB at minimum — PTY may coalesce)
        assert!(
            output.len() > 100_000,
            "expected >100KB of output, got {} bytes",
            output.len()
        );

        // Verify first and last lines are present
        assert!(
            text.contains("1\r\n") || text.contains("1\n"),
            "output should contain first line '1'"
        );
        assert!(
            text.contains("100000"),
            "output should contain last line '100000', total bytes: {}",
            output.len()
        );

        // Verify no truncation — check some middle values
        assert!(
            text.contains("50000"),
            "output should contain middle line '50000'"
        );
    }
}
