// Windows platform implementation — wraps existing ConPTY code behind platform traits.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use tracing::{debug, info, warn};

use crate::config::ToolKind;
use crate::conpty::{ConsoleMode, ConPtySession, OwnedHandle};
use crate::proxy::ShutdownReason;

use super::{PtyReader, PtySession, PtyWriter, TerminalMode};

// --- PtyReader / PtyWriter for OwnedHandle ---

struct WindowsPtyReader {
    handle: OwnedHandle,
}

impl PtyReader for WindowsPtyReader {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let n = self.handle.read(buf).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(n as usize)
    }
}

struct WindowsPtyWriter {
    handle: OwnedHandle,
}

impl PtyWriter for WindowsPtyWriter {
    fn write_all(&self, buf: &[u8]) -> Result<()> {
        self.handle.write_all(buf).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

// --- PtySession for ConPtySession ---

/// Windows PTY session wrapping ConPTY.
pub struct PlatformPtySession {
    inner: ConPtySession,
}

impl PtySession for PlatformPtySession {
    fn spawn(command: &str, cols: i16, rows: i16) -> Result<Self> {
        let inner = ConPtySession::spawn(command, cols, rows)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { inner })
    }

    fn take_io(&mut self) -> Option<(Box<dyn PtyWriter>, Box<dyn PtyReader>)> {
        let (input, output) = self.inner.take_io()?;
        Some((
            Box::new(WindowsPtyWriter { handle: input }),
            Box::new(WindowsPtyReader { handle: output }),
        ))
    }

    fn size(&self) -> (i16, i16) {
        self.inner.size()
    }

    fn resize(&mut self, cols: i16, rows: i16) -> Result<()> {
        self.inner.resize(cols, rows).map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn process_handle_raw(&self) -> usize {
        self.inner.process_handle_raw()
    }

    fn try_wait_for_child(&self, timeout_ms: u32) -> Result<Option<u32>> {
        self.inner.try_wait_for_child(timeout_ms).map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn process_id(&self) -> u32 {
        self.inner.process_id()
    }
}

// --- TerminalMode for ConsoleMode ---

/// Windows terminal mode wrapping ConsoleMode.
pub struct PlatformTerminalMode {
    inner: ConsoleMode,
}

impl TerminalMode for PlatformTerminalMode {
    fn save_and_set_raw() -> Result<Self> {
        let inner = ConsoleMode::save_and_set_raw()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self { inner })
    }

    fn restore(&self) -> Result<()> {
        self.inner.restore().map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn emergency_restore() {
        ConsoleMode::emergency_restore();
    }
}

// Allow the wrapper to be forgotten without triggering the inner Drop
impl PlatformTerminalMode {
    /// Consume self without running Drop on the inner ConsoleMode.
    /// Used after explicit restore() to prevent double-restore.
    pub fn forget(self) {
        std::mem::forget(self.inner);
        std::mem::forget(self);
    }

    /// Access the inner ConsoleMode for error recovery (e.g. restore before re-throwing).
    pub fn restore_and_forget(self) -> Result<()> {
        let result = self.inner.restore().map_err(|e| anyhow::anyhow!("{e}"));
        std::mem::forget(self.inner);
        std::mem::forget(self);
        result
    }
}

// --- Platform functions ---

pub fn get_terminal_size() -> Option<(i16, i16)> {
    crate::conpty::get_terminal_size()
}

pub fn raw_write_stdout(data: &[u8]) -> Result<()> {
    use windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
    use windows::Win32::Storage::FileSystem::WriteFile;

    let handle = unsafe {
        GetStdHandle(STD_OUTPUT_HANDLE).expect("failed to get stdout handle")
    };
    let mut data = data;
    while !data.is_empty() {
        let mut written = 0u32;
        unsafe {
            WriteFile(handle, Some(data), Some(&mut written), None)
                .map_err(|e| anyhow::anyhow!("WriteFile failed: {e}"))?;
        }
        data = &data[written as usize..];
    }
    Ok(())
}

pub fn is_stdin_interactive() -> bool {
    use windows::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE};

    unsafe {
        if let Ok(handle) = GetStdHandle(STD_INPUT_HANDLE) {
            let mut mode = windows::Win32::System::Console::CONSOLE_MODE(0);
            GetConsoleMode(handle, &mut mode).is_ok()
        } else {
            false
        }
    }
}

pub fn create_shutdown_signal() -> Result<usize> {
    use windows::Win32::System::Threading::CreateEventW;

    let handle = unsafe {
        CreateEventW(None, true, false, None)
            .context("CreateEventW failed")?
    };
    Ok(handle.0 as usize)
}

pub fn signal_shutdown(handle: usize) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::SetEvent;

    let handle = HANDLE(handle as *mut _);
    unsafe {
        let _ = SetEvent(handle);
    }
}

pub fn wait_for_child_exit(handle_raw: usize, exit_status: std::sync::Arc<std::sync::Mutex<Option<u32>>>) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{WaitForSingleObject, GetExitCodeProcess, INFINITE};

    let handle = HANDLE(handle_raw as *mut _);
    unsafe {
        WaitForSingleObject(handle, INFINITE);
        let mut code: u32 = 0;
        if GetExitCodeProcess(handle, &mut code).is_ok() {
            *exit_status.lock().unwrap() = Some(code);
        }
    }
}

pub fn run_console_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
    shutdown_event_handle: usize,
    resize_tx: Sender<(i16, i16)>,
    tool: ToolKind,
) {
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Console::{
        GetStdHandle, ReadConsoleInputW, INPUT_RECORD, KEY_EVENT, STD_INPUT_HANDLE,
        WINDOW_BUFFER_SIZE_EVENT,
    };
    use windows::Win32::System::Threading::WaitForMultipleObjects;

    use crate::proxy::key_translator::KeyTranslator;

    let stdin_handle = unsafe { GetStdHandle(STD_INPUT_HANDLE).unwrap_or_default() };
    let event_handle = HANDLE(shutdown_event_handle as *mut _);
    let handles = [stdin_handle, event_handle];
    let mut records = vec![INPUT_RECORD::default(); 128];
    let mut translator = KeyTranslator::new(tool);

    loop {
        if flag.load(Ordering::Relaxed) {
            break;
        }

        let wait_result = unsafe { WaitForMultipleObjects(&handles, false, 100) };

        if wait_result == WAIT_OBJECT_0 {
            let mut num_read = 0u32;
            let read_ok = unsafe {
                ReadConsoleInputW(stdin_handle, &mut records, &mut num_read).is_ok()
            };

            if !read_ok || num_read == 0 {
                info!("stdin EOF");
                let _ = shutdown_tx.try_send(ShutdownReason::InputEof);
                break;
            }

            let mut input_bytes = Vec::new();
            for record in &records[..num_read as usize] {
                match record.EventType as u32 {
                    KEY_EVENT => {
                        let key = unsafe { record.Event.KeyEvent };
                        if key.bKeyDown.as_bool() {
                            let uc = unsafe { key.uChar.UnicodeChar };
                            if uc != 0 {
                                if let Some(ch) = char::from_u32(uc as u32) {
                                    let mut buf = [0u8; 4];
                                    let encoded = ch.encode_utf8(&mut buf);
                                    for _ in 0..key.wRepeatCount.max(1) {
                                        input_bytes.extend_from_slice(encoded.as_bytes());
                                    }
                                }
                            }
                        }
                    }
                    WINDOW_BUFFER_SIZE_EVENT => {
                        let size = unsafe { record.Event.WindowBufferSizeEvent };
                        let new_cols = size.dwSize.X;
                        let new_rows = size.dwSize.Y;
                        debug!(cols = new_cols, rows = new_rows, "resize event received");
                        let _ = resize_tx.try_send((new_cols, new_rows));
                    }
                    _ => {
                        tracing::trace!(event_type = record.EventType, "skipping non-keyboard input event");
                    }
                }
            }

            if !input_bytes.is_empty() {
                let translated = translator.translate(&input_bytes);
                debug!(raw_bytes = input_bytes.len(), translated_bytes = translated.len(), "stdin read");
                if let Err(e) = input_write.write_all(&translated) {
                    if !flag.load(Ordering::Relaxed) {
                        warn!(error = %e, "input pipe write error");
                        let _ = shutdown_tx.try_send(ShutdownReason::IoError(e.to_string()));
                    }
                    break;
                }
            }
        } else if wait_result.0 == WAIT_OBJECT_0.0 + 1 {
            info!("input thread: shutdown event received");
            break;
        }
    }

    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(event_handle);
    }
}

pub fn run_pipe_input_loop(
    input_write: Box<dyn PtyWriter>,
    flag: Arc<AtomicBool>,
    shutdown_tx: Sender<ShutdownReason>,
) {
    use std::io::Read;

    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let mut buf = vec![0u8; 1024];

    loop {
        if flag.load(Ordering::Relaxed) {
            break;
        }
        match stdin.read(&mut buf) {
            Ok(0) => {
                info!("stdin EOF");
                let _ = shutdown_tx.try_send(ShutdownReason::InputEof);
                break;
            }
            Ok(n) => {
                debug!(bytes = n, "stdin read");
                if let Err(e) = input_write.write_all(&buf[..n]) {
                    if !flag.load(Ordering::Relaxed) {
                        warn!(error = %e, "input pipe write error");
                        let _ = shutdown_tx.try_send(ShutdownReason::IoError(e.to_string()));
                    }
                    break;
                }
            }
            Err(e) => {
                if !flag.load(Ordering::Relaxed) {
                    warn!(error = %e, "stdin read error");
                    let _ = shutdown_tx.try_send(ShutdownReason::IoError(e.to_string()));
                }
                break;
            }
        }
    }
}

/// Check if a spawn error is a known Windows error and print a friendly message.
/// Returns true if a friendly message was printed.
pub fn print_friendly_spawn_error(command: &str, error: &anyhow::Error) -> bool {
    for cause in error.chain() {
        if let Some(win_err) = cause.downcast_ref::<windows::core::Error>() {
            let code = win_err.code().0 as u32;
            match code {
                0x80070002 => {
                    eprintln!("error: '{command}' not found.");
                    eprintln!("  Make sure it's installed and on your PATH.");
                    eprintln!("  Run 'where {command}' to check.");
                    return true;
                }
                0x80070005 => {
                    eprintln!("error: Permission denied when launching '{command}'.");
                    eprintln!("  Try running as administrator.");
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}
