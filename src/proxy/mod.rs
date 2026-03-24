// Proxy module — the main event loop
//
// Coordinates:
// - PTY I/O threads (input + output)
// - Sync block detection
// - History management
//
// Architecture:
//   Input thread:  Real stdin → PTY input (+ resize events → main thread)
//   Output thread: PTY output → channel → main thread
//   Watcher thread: wait for child process exit → shutdown signal
//   Main thread:   Sync detector → stdout passthrough + history + metrics
//
// Phase 1 strategy: transparent passthrough with instrumentation.
// All child output goes directly to stdout. The sync detector and history
// still process data for metrics and future Phase 2 use. Differential
// rendering is deferred to Phase 2 (Tauri terminal) where we control
// the display surface.
//
// Shutdown sequence:
//   1. Child process exits → watcher thread sends ChildExited signal
//   2. Main loop breaks
//   3. Session is dropped (closes PTY) — output pipe gets EOF
//   4. Output thread exits
//   5. Input thread is signaled via shutdown mechanism

pub mod events;
pub mod key_translator;
pub mod output_sink;
#[cfg(feature = "recording")]
#[deny(dead_code)]
pub mod recorder;
pub mod render_coalescer;

#[allow(unused_imports)] // Phase 2 — re-export used by Tauri GUI
pub use output_sink::OutputSink;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, select, Receiver, Sender};
use tracing::{debug, error, info, warn};

use crate::config::{AppConfig, ToolKind};
use crate::history::{HistoryEventType, LineBuffer, OutputFilter};
use crate::platform::{self, PtySession};
use crate::vt::{SyncBlockDetector, SyncEvent};

use events::{event_channel, ProxyEvent};
use key_translator::{KITTY_DISABLE, KITTY_ENABLE};

/// Reason a thread is signaling shutdown
#[derive(Debug, Clone)]
pub(crate) enum ShutdownReason {
    InputEof,
    OutputEof,
    ChildExited,
    IoError(#[allow(dead_code)] String),
}

/// The proxy coordinator. Owns all processing state and runs the main loop.
pub struct Proxy {
    config: AppConfig,
    tool: ToolKind,
    session: platform::PlatformPtySession,
    event_tx: Sender<ProxyEvent>,
    #[cfg(feature = "recording")]
    recorder: Option<recorder::VtcapRecorder>,
}

impl Proxy {
    /// Create a new proxy. Returns (proxy, event_receiver).
    /// In Phase 1, the caller can drop the receiver immediately.
    pub fn new(config: AppConfig, tool: ToolKind, session: platform::PlatformPtySession) -> (Self, Receiver<ProxyEvent>) {
        let (event_tx, event_rx) = event_channel();
        let (cols, rows) = session.size();
        info!(cols, rows, tool = %tool, "proxy created");
        (
            Self {
                config,
                tool,
                session,
                event_tx,
                #[cfg(feature = "recording")]
                recorder: None,
            },
            event_rx,
        )
    }

    /// Attach a VtcapRecorder to capture filtered output during the session.
    #[cfg(feature = "recording")]
    pub fn with_recorder(mut self, recorder: recorder::VtcapRecorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Run the proxy. Blocks until the child exits or an error occurs.
    /// Returns the child's exit code.
    pub fn run(mut self) -> Result<u32> {
        // Take I/O handles from session
        let (input_write, output_read) = self
            .session
            .take_io()
            .context("failed to take I/O handles from session")?;

        // Create pipeline components
        let (cols, rows) = self.session.size();
        let mut output_filter = OutputFilter::new();
        let mut detector = SyncBlockDetector::new();
        let mut history = LineBuffer::new(self.config.history_lines);
        let mut total_bytes: u64 = 0;
        let mut chunk_count: u64 = 0;

        // Channels
        let (output_tx, output_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = bounded(64);
        let (shutdown_tx, shutdown_rx): (Sender<ShutdownReason>, Receiver<ShutdownReason>) =
            bounded(4);
        let (resize_tx, resize_rx) = bounded::<(i16, i16)>(4);

        let shutdown_flag = Arc::new(AtomicBool::new(false));

        // No Ctrl+C handler — in raw mode, Ctrl+C arrives as byte 0x03 via stdin,
        // which our input thread writes to the PTY's input. The PTY then generates
        // the appropriate signal for the child process.

        // Shared exit status: the child-watcher thread stores the exit code here
        // before signaling shutdown, avoiding a race where the main loop's waitpid
        // gets ECHILD because the watcher already reaped the child.
        let child_exit_status: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));

        // Child exit watcher thread: detects when the child process exits.
        let child_process_raw = self.session.process_handle_raw();
        let watcher_shutdown_tx = shutdown_tx.clone();
        let watcher_exit_status = child_exit_status.clone();
        thread::Builder::new()
            .name("child-watcher".into())
            .spawn(move || {
                platform::wait_for_child_exit(child_process_raw, watcher_exit_status);
                info!("child process exited (watcher)");
                let _ = watcher_shutdown_tx.try_send(ShutdownReason::ChildExited);
            })
            .context("failed to spawn child watcher thread")?;
        info!("child watcher thread started");

        // Output thread: reads from PTY output, sends to main thread
        let output_shutdown_tx = shutdown_tx.clone();
        let output_flag = shutdown_flag.clone();
        let output_thread = thread::Builder::new()
            .name("pty-output".into())
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
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            if !output_flag.load(Ordering::Relaxed) {
                                warn!(error = %e, "output pipe read error");
                                let _ = output_shutdown_tx
                                    .try_send(ShutdownReason::IoError(e.to_string()));
                            }
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn output thread")?;
        info!("output thread started");

        // Input thread: reads from real stdin, writes to PTY input.
        let input_shutdown_tx = shutdown_tx.clone();
        let input_flag = shutdown_flag.clone();

        // Create a shutdown signaling mechanism for the input thread
        let shutdown_signal = platform::create_shutdown_signal()
            .context("failed to create shutdown signal")?;

        let stdin_is_interactive = platform::is_stdin_interactive();
        debug!(stdin_is_interactive, "input thread mode selected");

        let tool = self.tool;
        let input_thread = thread::Builder::new()
            .name("pty-input".into())
            .spawn(move || {
                if stdin_is_interactive {
                    platform::run_console_input_loop(
                        input_write,
                        input_flag,
                        input_shutdown_tx,
                        shutdown_signal,
                        resize_tx,
                        tool,
                    );
                } else {
                    platform::run_pipe_input_loop(input_write, input_flag, input_shutdown_tx);
                }
            })
            .context("failed to spawn input thread")?;
        info!("input thread started");

        let mut last_size = (cols, rows);

        // Enable Kitty keyboard protocol on the outer terminal.
        // Terminals that don't support it will ignore the sequence.
        if let Err(e) = platform::raw_write_stdout(KITTY_ENABLE) {
            warn!(error = %e, "failed to send Kitty protocol enable");
        } else {
            info!("Kitty keyboard protocol enable sent");
        }

        info!("entering main proxy loop (passthrough mode)");

        loop {
            select! {
                recv(output_rx) -> msg => {
                    match msg {
                        Ok(data) => {
                            // Filter dangerous sequences before output
                            let filtered = output_filter.filter(&data);

                            // Record post-filter data for replay testing
                            #[cfg(feature = "recording")]
                            if let Some(ref mut rec) = self.recorder
                                && let Err(e) = rec.write_chunk(filtered) {
                                warn!(error = %e, "recording failed, disabling");
                                self.recorder = None;
                            }

                            let filtered_owned = filtered.to_vec();
                            if let Err(e) = platform::raw_write_stdout(&filtered_owned) {
                                error!(error = %e, "failed to write to stdout");
                                break;
                            }

                            // Feed sync detector for history/metrics
                            let events = detector.process(&filtered_owned);

                            // Feed history from detector events
                            for event in &events {
                                match event {
                                    SyncEvent::PassThrough(bytes) => {
                                        history.push(bytes, HistoryEventType::Output);
                                    }
                                    SyncEvent::SyncBlock { data: block_data, is_full_redraw } => {
                                        if *is_full_redraw {
                                            history.insert_boundary(HistoryEventType::FullRedrawBoundary);
                                        }
                                        history.push(block_data, HistoryEventType::SyncBlock);

                                        let _ = self.event_tx.try_send(
                                            ProxyEvent::SyncBlockComplete {
                                                size_bytes: block_data.len(),
                                                is_full_redraw: *is_full_redraw,
                                            }
                                        );
                                    }
                                }
                            }

                            total_bytes += filtered_owned.len() as u64;
                            chunk_count += 1;
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
                recv(resize_rx) -> msg => {
                    if let Ok((new_cols, new_rows)) = msg
                        && (new_cols, new_rows) != last_size
                    {
                        info!(
                            old_cols = last_size.0,
                            old_rows = last_size.1,
                            new_cols,
                            new_rows,
                            "terminal resize detected"
                        );
                        if let Err(e) = self.session.resize(new_cols, new_rows) {
                            warn!(error = %e, "failed to resize PTY");
                        }
                        last_size = (new_cols, new_rows);

                        let _ = self.event_tx.try_send(ProxyEvent::Resize {
                            cols: new_cols,
                            rows: new_rows,
                        });
                    }
                }
            }
        }

        // Disable Kitty keyboard protocol before restoring terminal state
        if let Err(e) = platform::raw_write_stdout(KITTY_DISABLE) {
            warn!(error = %e, "failed to send Kitty protocol disable");
        } else {
            info!("Kitty keyboard protocol disabled");
        }

        // Finalize recording if active
        #[cfg(feature = "recording")]
        if let Some(rec) = self.recorder.take()
            && let Err(e) = rec.finish() {
            warn!(error = %e, "failed to finalize vtcap recording");
        }

        // Signal all threads to stop
        info!("shutting down I/O threads");
        shutdown_flag.store(true, Ordering::Relaxed);

        // Signal the input thread to wake up
        platform::signal_shutdown(shutdown_signal);

        // Get exit code: prefer the status stored by the child-watcher thread
        // (which already reaped the child via blocking waitpid). Fall back to
        // try_wait_for_child only if the watcher hasn't stored a status yet
        // (e.g., shutdown was triggered by something other than child exit).
        let exit_code = {
            let stored = child_exit_status.lock().unwrap().take();
            if let Some(code) = stored {
                info!(exit_code = code, "child exited (from watcher)");
                let _ = self.event_tx.try_send(ProxyEvent::ChildExited {
                    exit_code: code,
                });
                code
            } else {
                match self.session.try_wait_for_child(2000) {
                    Ok(Some(code)) => {
                        info!(exit_code = code, "child exited (from try_wait)");
                        let _ = self.event_tx.try_send(ProxyEvent::ChildExited {
                            exit_code: code,
                        });
                        code
                    }
                    Ok(None) => {
                        warn!("child did not exit within timeout");
                        0
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to get child exit code");
                        0
                    }
                }
            }
        };

        // Drop session — this closes the PTY, causing the output thread to get EOF
        info!("closing PTY session");
        drop(self.session);

        // Wait for the output thread — it should exit quickly after pipe EOF
        info!("waiting for output thread");
        let _ = output_thread.join();

        // The input thread is NOT joined — it may be blocked in a read.
        // The thread will be killed when the process exits.
        drop(input_thread);

        info!(
            total_bytes,
            chunk_count,
            "proxy shutdown complete"
        );

        Ok(exit_code)
    }
}

/// Strip CSI 2J (erase display) and cursor-home sequences from a sync block.
/// These sequences cause the terminal to reset scroll position. By removing them
/// from full-redraw sync blocks, the content update happens without scroll jumping.
///
/// Handles all cursor-home variants: ESC[H, ESC[;H, ESC[1;1H, ESC[1H
#[allow(dead_code)] // Used by replay tests and output filter
pub fn strip_clear_screen(data: &[u8]) -> Vec<u8> {
    use memchr::memmem;

    let clear_screen = b"\x1b[2J";

    let mut result = data.to_vec();

    // Strip all occurrences of CSI 2J
    while let Some(pos) = memmem::find(&result, clear_screen) {
        result.drain(pos..pos + clear_screen.len());
    }

    // Strip cursor-home variants only at position 0 — they're often paired with
    // clear screen. Don't strip cursor-home elsewhere as it may be part of
    // legitimate content positioning.
    for pattern in &[
        &b"\x1b[1;1H"[..],
        &b"\x1b[;H"[..],
        &b"\x1b[1H"[..],
        &b"\x1b[H"[..],
    ] {
        if result.starts_with(pattern) {
            result.drain(..pattern.len());
            break;
        }
    }

    result
}

#[cfg(test)]
mod strip_tests {
    use super::*;

    #[test]
    fn test_strip_clear_screen_and_cursor_home() {
        let input = b"\x1b[2J\x1b[Hscreen content here";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"screen content here");
    }

    #[test]
    fn test_strip_clear_screen_only() {
        let input = b"\x1b[2Jcontent";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"content");
    }

    #[test]
    fn test_no_clear_screen_unchanged() {
        let input = b"\x1b[31mred text\x1b[0m";
        let result = strip_clear_screen(input);
        assert_eq!(result, input.to_vec());
    }

    #[test]
    fn test_cursor_home_mid_content_preserved() {
        // CSI H in the middle of content should be preserved
        let input = b"before\x1b[Hafter";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"before\x1b[Hafter");
    }

    #[test]
    fn test_multiple_clear_screens_stripped() {
        let input = b"\x1b[2Jfirst\x1b[2Jsecond";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"firstsecond");
    }

    #[test]
    fn test_empty_input() {
        let result = strip_clear_screen(b"");
        assert!(result.is_empty());
    }

    #[test]
    fn test_strip_cursor_home_variant_1_1() {
        let input = b"\x1b[2J\x1b[1;1Hscreen content";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"screen content");
    }

    #[test]
    fn test_strip_cursor_home_variant_semicolon() {
        let input = b"\x1b[2J\x1b[;Hscreen content";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"screen content");
    }

    #[test]
    fn test_strip_cursor_home_variant_1() {
        let input = b"\x1b[2J\x1b[1Hscreen content";
        let result = strip_clear_screen(input);
        assert_eq!(result, b"screen content");
    }
}
