//! Save and restore the parent terminal's console mode.
//!
//! When running as a proxy, we need to enable VT processing on stdout
//! and VT input on stdin. On exit we restore the original modes.

use std::sync::OnceLock;

use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_INPUT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
    GetConsoleCP, GetConsoleOutputCP, SetConsoleCP, SetConsoleOutputCP,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::Foundation::HANDLE;
use tracing::{info, warn};

use super::error::Result;
use super::sys;

const CP_UTF8: u32 = 65001;

/// Static storage for emergency restore from panic hooks.
/// Stores (stdin_handle_raw, stdout_handle_raw, original_stdin_mode, original_stdout_mode,
///         original_input_cp, original_output_cp).
static SAVED_MODES: OnceLock<(usize, usize, u32, u32, u32, u32)> = OnceLock::new();

/// Saved console modes and codepages for stdin and stdout.
/// Restores original modes on drop.
pub struct ConsoleMode {
    stdin_handle: HANDLE,
    stdout_handle: HANDLE,
    original_stdin_mode: CONSOLE_MODE,
    original_stdout_mode: CONSOLE_MODE,
    original_input_cp: u32,
    original_output_cp: u32,
}

impl ConsoleMode {
    /// Save current console modes and set raw/VT modes for proxy operation.
    ///
    /// stdin: enables virtual terminal input + window input events
    /// stdout: enables virtual terminal processing (pass-through VT sequences)
    pub fn save_and_set_raw() -> Result<Self> {
        let stdin_handle = sys::get_std_handle(STD_INPUT_HANDLE)?;
        let stdout_handle = sys::get_std_handle(STD_OUTPUT_HANDLE)?;

        let original_stdin_mode = sys::get_console_mode(stdin_handle)?;
        let original_stdout_mode = sys::get_console_mode(stdout_handle)?;

        // Save current codepages — ConPTY outputs UTF-8, so we need CP 65001
        let original_input_cp = unsafe { GetConsoleCP() };
        let original_output_cp = unsafe { GetConsoleOutputCP() };

        info!(
            stdin_mode = original_stdin_mode.0,
            stdout_mode = original_stdout_mode.0,
            input_cp = original_input_cp,
            output_cp = original_output_cp,
            "console modes saved"
        );

        // Store in static for emergency restore
        SAVED_MODES.get_or_init(|| {
            (
                stdin_handle.0 as usize,
                stdout_handle.0 as usize,
                original_stdin_mode.0,
                original_stdout_mode.0,
                original_input_cp,
                original_output_cp,
            )
        });

        // Set stdin to raw VT input mode.
        // ENABLE_PROCESSED_INPUT is intentionally omitted — with it enabled,
        // Ctrl+C is intercepted by the console subsystem and never reaches our
        // stdin read loop. Without it, Ctrl+C arrives as byte 0x03, which our
        // input thread writes to ConPTY's pipe, and ConPTY generates the
        // CTRL_C_EVENT for the child process naturally.
        let new_stdin_mode = ENABLE_VIRTUAL_TERMINAL_INPUT
            | ENABLE_WINDOW_INPUT;
        sys::set_console_mode(stdin_handle, new_stdin_mode)?;

        // Set stdout to VT processing mode
        let new_stdout_mode = original_stdout_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        sys::set_console_mode(stdout_handle, new_stdout_mode)?;

        // Set codepages to UTF-8 — ConPTY outputs UTF-8 encoded VT sequences,
        // and the console must interpret them as UTF-8 to render multi-byte
        // characters (box-drawing, Unicode) correctly.
        unsafe {
            if SetConsoleCP(CP_UTF8).is_err() {
                warn!("failed to set input codepage to UTF-8");
            }
            if SetConsoleOutputCP(CP_UTF8).is_err() {
                warn!("failed to set output codepage to UTF-8");
            }
        }

        info!(
            new_stdin_mode = new_stdin_mode.0,
            new_stdout_mode = new_stdout_mode.0,
            "console modes set for proxy operation (codepage: UTF-8)"
        );

        Ok(Self {
            stdin_handle,
            stdout_handle,
            original_stdin_mode,
            original_stdout_mode,
            original_input_cp,
            original_output_cp,
        })
    }

    /// Restore original console modes and codepages.
    pub fn restore(&self) -> Result<()> {
        sys::set_console_mode(self.stdin_handle, self.original_stdin_mode)?;
        sys::set_console_mode(self.stdout_handle, self.original_stdout_mode)?;
        unsafe {
            let _ = SetConsoleCP(self.original_input_cp);
            let _ = SetConsoleOutputCP(self.original_output_cp);
        }
        info!("console modes restored");
        Ok(())
    }

    /// Emergency restore from a panic hook or signal handler.
    ///
    /// Uses the statically-saved handle values and modes. Safe to call
    /// from any thread, any context. Silently does nothing if modes were
    /// never saved.
    pub fn emergency_restore() {
        use windows::Win32::System::Console::SetConsoleMode;

        if let Some(&(stdin_raw, stdout_raw, stdin_mode, stdout_mode, input_cp, output_cp)) =
            SAVED_MODES.get()
        {
            let stdin_h = HANDLE(stdin_raw as *mut _);
            let stdout_h = HANDLE(stdout_raw as *mut _);
            unsafe {
                let _ = SetConsoleMode(stdin_h, CONSOLE_MODE(stdin_mode));
                let _ = SetConsoleMode(stdout_h, CONSOLE_MODE(stdout_mode));
                let _ = SetConsoleCP(input_cp);
                let _ = SetConsoleOutputCP(output_cp);
            }
        }
    }
}

impl Drop for ConsoleMode {
    fn drop(&mut self) {
        if let Err(e) = self.restore() {
            warn!(error = %e, "failed to restore console mode in drop");
        }
    }
}
