// Proxy module — the main event loop
//
// Coordinates:
// - ConPTY I/O threads (input + output)
// - Sync block detection
// - VT differential rendering
// - History management
// - Render coalescing
//
// Architecture:
//   Input thread:  Real stdin → ConPTY input pipe
//   Output thread: ConPTY output pipe → channel → main thread
//   Main thread:   Sync detector → VT emulator → Diff renderer → Real stdout

pub mod events;
pub mod render_coalescer;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, select, tick, Receiver, Sender};
use tracing::{debug, error, info, trace, warn};

use crate::config::AppConfig;
use crate::conpty::ConPtySession;
use crate::history::LineBuffer;
use crate::vt::{DiffRenderer, SyncBlockDetector, SyncEvent};

use events::{event_channel, ProxyEvent};
use render_coalescer::RenderCoalescer;

/// Reason a thread is signaling shutdown
#[derive(Debug, Clone)]
enum ShutdownReason {
    InputEof,
    OutputEof,
    CtrlC,
    IoError(String),
}

/// The proxy coordinator. Owns all processing state and runs the main loop.
pub struct Proxy {
    config: AppConfig,
    session: ConPtySession,
    event_tx: Sender<ProxyEvent>,
}

impl Proxy {
    /// Create a new proxy. Returns (proxy, event_receiver).
    /// In Phase 1, the caller can drop the receiver immediately.
    pub fn new(config: AppConfig, session: ConPtySession) -> (Self, Receiver<ProxyEvent>) {
        let (event_tx, event_rx) = event_channel();
        let (cols, rows) = session.size();
        info!(cols, rows, "proxy created");
        (
            Self {
                config,
                session,
                event_tx,
            },
            event_rx,
        )
    }

    /// Run the proxy. Blocks until the child exits or an error occurs.
    /// Returns the child's exit code.
    pub fn run(mut self) -> Result<u32> {
        // Take I/O handles from session
        let (input_write, output_read) = self
            .session
            .take_io()
            .context("failed to take I/O handles from session")?;

        // Create processing pipeline components
        let (cols, rows) = self.session.size();
        let mut detector = SyncBlockDetector::new();
        let mut renderer = DiffRenderer::new(rows as u16, cols as u16);
        let mut history = LineBuffer::new(self.config.history_lines);
        let mut coalescer = RenderCoalescer::new(
            Duration::from_millis(self.config.render_delay_ms),
            Duration::from_millis(self.config.sync_delay_ms),
            Duration::from_nanos(16_666_667), // ~60fps
        );

        // Channels
        let (output_tx, output_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = bounded(64);
        let (shutdown_tx, shutdown_rx): (Sender<ShutdownReason>, Receiver<ShutdownReason>) =
            bounded(4);

        let shutdown_flag = Arc::new(AtomicBool::new(false));

        // Set up Ctrl+C handler
        {
            let stx = shutdown_tx.clone();
            let flag = shutdown_flag.clone();
            install_ctrlc_handler(stx, flag);
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
                                    .try_send(ShutdownReason::IoError(e.to_string()));
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
                                        .try_send(ShutdownReason::IoError(e.to_string()));
                                }
                                break;
                            }
                        }
                        Err(e) => {
                            if !input_flag.load(Ordering::Relaxed) {
                                warn!(error = %e, "stdin read error");
                                let _ = input_shutdown_tx
                                    .try_send(ShutdownReason::IoError(e.to_string()));
                            }
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn input thread")?;

        // Resize polling ticker
        let resize_tick = tick(Duration::from_millis(100));

        let mut stdout = std::io::stdout().lock();
        let mut last_size = (cols, rows);
        let mut frame_number: u64 = 0;

        info!("entering main proxy loop (differential rendering active)");

        loop {
            // Compute timeout for select: either the coalescer deadline or a long default
            let timeout = coalescer
                .time_until_render()
                .unwrap_or(Duration::from_millis(100));

            select! {
                recv(output_rx) -> msg => {
                    match msg {
                        Ok(data) => {
                            // Process through sync detector
                            let events = detector.process(&data);
                            for event in events {
                                match event {
                                    SyncEvent::PassThrough(bytes) => {
                                        renderer.feed(bytes);
                                        history.push(bytes);
                                        coalescer.notify_data();
                                    }
                                    SyncEvent::SyncBlock { data: block_data, is_full_redraw } => {
                                        renderer.feed(&block_data);
                                        history.push(&block_data);
                                        coalescer.notify_sync_block();

                                        let _ = self.event_tx.try_send(
                                            ProxyEvent::SyncBlockComplete {
                                                size_bytes: block_data.len(),
                                                is_full_redraw,
                                            }
                                        );
                                    }
                                }
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
                    if let Some((new_cols, new_rows)) = crate::conpty::get_terminal_size() {
                        if (new_cols, new_rows) != last_size {
                            info!(
                                old_cols = last_size.0,
                                old_rows = last_size.1,
                                new_cols,
                                new_rows,
                                "terminal resize detected"
                            );
                            if let Err(e) = self.session.resize(new_cols, new_rows) {
                                warn!(error = %e, "failed to resize ConPTY");
                            }
                            renderer.resize(new_rows as u16, new_cols as u16);
                            last_size = (new_cols, new_rows);

                            let _ = self.event_tx.try_send(ProxyEvent::Resize {
                                cols: new_cols,
                                rows: new_rows,
                            });
                        } else {
                            trace!("resize poll: size unchanged");
                        }
                    }
                }
                default(timeout) => {
                    // Timeout expired — check if we should render
                    trace!("select timeout (coalescer check)");
                }
            }

            // After every select! arm: try to render if coalescer says it's time
            if coalescer.should_render() && renderer.is_dirty() {
                if let Some(diff) = renderer.render() {
                    let diff_len = diff.len();
                    if let Err(e) = stdout.write_all(&diff) {
                        error!(error = %e, "failed to write to stdout");
                        break;
                    }
                    if let Err(e) = stdout.flush() {
                        error!(error = %e, "failed to flush stdout");
                        break;
                    }

                    frame_number += 1;
                    debug!(
                        frame = frame_number,
                        diff_bytes = diff_len,
                        "frame rendered"
                    );

                    let _ = self.event_tx.try_send(ProxyEvent::RenderComplete {
                        output_bytes: diff_len,
                        diff_bytes: diff_len,
                        frame_number,
                    });
                }
                coalescer.mark_rendered();
            }
        }

        // Final flush: render any remaining dirty state
        if renderer.is_dirty()
            && let Some(diff) = renderer.render()
        {
            let _ = stdout.write_all(&diff);
            let _ = stdout.flush();
            frame_number += 1;
            debug!(frame = frame_number, "final frame rendered");
        }

        // Signal all threads to stop
        info!("shutting down I/O threads");
        shutdown_flag.store(true, Ordering::Relaxed);

        // Wait for child exit code before dropping session
        let exit_code = match self.session.wait_for_child() {
            Ok(code) => {
                info!(exit_code = code, "child exited");
                let _ = self.event_tx.try_send(ProxyEvent::ChildExited {
                    exit_code: code,
                });
                code
            }
            Err(e) => {
                warn!(error = %e, "failed to get child exit code");
                0
            }
        };

        // Drop session to close ConPTY — unblocks the output thread
        drop(self.session);

        // Join output thread
        let _ = output_thread.join();

        // Log final metrics
        let sync_metrics = detector.metrics();
        let diff_metrics = renderer.metrics();
        let hist_metrics = history.metrics();
        info!(
            frames = frame_number,
            sync_blocks = sync_metrics.sync_blocks_detected,
            full_redraws = sync_metrics.full_redraws_detected,
            diff_renders = diff_metrics.diff_renders,
            full_renders = diff_metrics.full_renders,
            compression_pct = format!("{:.1}", diff_metrics.compression_ratio() * 100.0),
            history_lines = hist_metrics.current_size,
            "proxy shutdown complete"
        );

        Ok(exit_code)
    }
}

fn install_ctrlc_handler(shutdown_tx: Sender<ShutdownReason>, flag: Arc<AtomicBool>) {
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
