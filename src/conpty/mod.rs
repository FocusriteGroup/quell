// ConPTY session management — Windows pseudoconsole API wrapper
//
// This module handles:
// - Creating ConPTY sessions via CreatePseudoConsole
// - Spawning child processes attached to the pseudoconsole
// - Managing input/output pipes (separate threads required)
// - Resize handling via ResizePseudoConsole
// - Console mode save/restore for the parent terminal

pub mod error;
pub mod console_mode;
mod pipes;
pub mod session;
mod sys;

pub use console_mode::ConsoleMode;
pub use error::ConPtyError;
pub use pipes::OwnedHandle;
pub use session::ConPtySession;

/// Detect the current terminal size from the real stdout console.
/// Returns (cols, rows) or None if not attached to a console.
pub fn get_terminal_size() -> Option<(i16, i16)> {
    use windows::Win32::System::Console::STD_OUTPUT_HANDLE;
    let handle = sys::get_std_handle(STD_OUTPUT_HANDLE).ok()?;
    let info = sys::get_console_screen_buffer_info(handle).ok()?;
    let cols = info.srWindow.Right - info.srWindow.Left + 1;
    let rows = info.srWindow.Bottom - info.srWindow.Top + 1;
    Some((cols, rows))
}
