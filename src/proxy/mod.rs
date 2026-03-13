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
//   Output thread: ConPTY output pipe → Sync detector → VT emulator → Diff renderer → Real stdout
//   Main thread:   Coordinates rendering timing, resize events, signals

use tracing::info;

/// Placeholder for the proxy coordinator
pub struct Proxy {
    // Will contain:
    // - ConPTY session
    // - DiffRenderer
    // - SyncBlockDetector
    // - LineBuffer (history)
    // - RenderCoalescer
    // - Thread handles
}

impl Proxy {
    pub fn new() -> Self {
        info!("proxy module loaded (not yet implemented)");
        Self {}
    }
}
