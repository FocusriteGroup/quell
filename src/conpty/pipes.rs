//! Pipe management for ConPTY sessions.
//!
//! Provides owned handles that clean up on drop, plus helpers for
//! reading and writing through the ConPTY pipes.

use windows::Win32::Foundation::HANDLE;
use tracing::{debug, warn};

use super::error::Result;
use super::sys;

/// An owned Win32 HANDLE that closes on drop.
pub struct OwnedHandle {
    handle: HANDLE,
}

impl OwnedHandle {
    /// Wrap a raw HANDLE. The handle will be closed on drop.
    pub(super) fn new(handle: HANDLE) -> Self {
        Self { handle }
    }

    /// Get the raw HANDLE value.
    #[allow(dead_code)] // Phase 2 extension point
    pub fn raw(&self) -> HANDLE {
        self.handle
    }

    /// Read bytes into the buffer. Returns the number of bytes read.
    /// Returns 0 on EOF.
    pub fn read(&self, buf: &mut [u8]) -> Result<u32> {
        let n = sys::read_file(self.handle, buf)?;
        debug!(bytes = n, "pipe read");
        Ok(n)
    }

    /// Write all bytes to the handle. Loops until all bytes are written.
    pub fn write_all(&self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = sys::write_file(self.handle, buf)?;
            debug!(bytes = n, "pipe write");
            if n == 0 {
                warn!("pipe write returned 0 bytes");
                break;
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        sys::close_handle(self.handle);
    }
}

// OwnedHandle is Send — we transfer ownership to I/O threads
unsafe impl Send for OwnedHandle {}

/// Create the four pipes needed for a ConPTY session.
///
/// Returns `(conpty_input_read, proxy_input_write, proxy_output_read, conpty_output_write)`:
/// - `conpty_input_read` + `conpty_output_write`: given to CreatePseudoConsole
/// - `proxy_input_write`: our side writes stdin to the child through this
/// - `proxy_output_read`: our side reads child output from this
pub(super) fn create_conpty_pipes() -> Result<(HANDLE, OwnedHandle, OwnedHandle, HANDLE)> {
    // Input pipe: proxy writes → ConPTY reads
    let (input_read, input_write) = sys::create_pipe()?;
    // Output pipe: ConPTY writes → proxy reads
    let (output_read, output_write) = sys::create_pipe()?;

    Ok((
        input_read,
        OwnedHandle::new(input_write),
        OwnedHandle::new(output_read),
        output_write,
    ))
}
