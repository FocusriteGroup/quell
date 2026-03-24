#![allow(dead_code)] // Phase 2 extension point — used by tests + Tauri GUI, not the CLI binary yet
// Output sink abstraction — decouples the proxy from stdout.
//
// Phase 1 (CLI proxy) uses StdoutSink which writes directly to stdout
// with Kitty keyboard protocol management.
// Phase 2 (Tauri GUI) will use TauriIpcSink which emits events to the frontend.

use anyhow::Result;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Trait for receiving proxy output. Implementations control where
/// processed terminal data goes — stdout, IPC channel, test buffer, etc.
pub trait OutputSink: Send {
    /// Write terminal output data.
    fn write(&self, data: &[u8]) -> Result<()>;

    /// Called once when the proxy starts. StdoutSink uses this to enable
    /// Kitty keyboard protocol; GUI sinks can no-op.
    fn on_startup(&self) {}

    /// Called once when the proxy shuts down. StdoutSink uses this to disable
    /// Kitty keyboard protocol; GUI sinks can no-op.
    fn on_shutdown(&self) {}
}

/// Writes directly to stdout using platform-specific raw write.
/// Manages Kitty keyboard protocol enable/disable on startup/shutdown.
pub struct StdoutSink;

impl StdoutSink {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self
    }
}

impl OutputSink for StdoutSink {
    fn write(&self, data: &[u8]) -> Result<()> {
        crate::platform::raw_write_stdout(data)
    }

    fn on_startup(&self) {
        use super::key_translator::KITTY_ENABLE;
        if let Err(e) = crate::platform::raw_write_stdout(KITTY_ENABLE) {
            warn!(error = %e, "failed to send Kitty protocol enable");
        } else {
            info!("Kitty keyboard protocol enable sent");
        }
    }

    fn on_shutdown(&self) {
        use super::key_translator::KITTY_DISABLE;
        if let Err(e) = crate::platform::raw_write_stdout(KITTY_DISABLE) {
            warn!(error = %e, "failed to send Kitty protocol disable");
        } else {
            info!("Kitty keyboard protocol disabled");
        }
    }
}

/// Captures output into a shared buffer. Useful for testing.
pub struct BufferSink {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl BufferSink {
    pub fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        (Self { buffer: buffer.clone() }, buffer)
    }
}

impl OutputSink for BufferSink {
    fn write(&self, data: &[u8]) -> Result<()> {
        self.buffer.lock().unwrap().extend_from_slice(data);
        Ok(())
    }
}
