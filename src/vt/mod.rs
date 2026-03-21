mod sync_detector;
mod diff_renderer;

pub use sync_detector::{SyncBlockDetector, SyncEvent};
#[allow(unused_imports)] // Phase 2 — used by Tauri GUI
pub use diff_renderer::DiffRenderer;
