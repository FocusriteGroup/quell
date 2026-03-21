mod line_buffer;
mod escape_filter;
mod output_filter;

#[allow(unused_imports)] // Phase 2 — re-exports used by tests + Tauri GUI
pub use line_buffer::{HistoryEntry, HistoryEventType, LineBuffer};
#[allow(unused_imports)] // Phase 2
pub use escape_filter::EscapeFilter;
pub use output_filter::OutputFilter;
