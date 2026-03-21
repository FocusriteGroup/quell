use thiserror::Error;

/// Errors from ConPTY session management
#[derive(Error, Debug)]
pub enum ConPtyError {
    #[error("failed to create pipes: {source}")]
    PipeCreation {
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to create pseudo console ({cols}x{rows}): {source}")]
    PseudoConsoleCreation {
        cols: i16,
        rows: i16,
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to spawn process '{command}': {source}")]
    ProcessSpawn {
        command: String,
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to resize pseudo console to {cols}x{rows}: {source}")]
    Resize {
        cols: i16,
        rows: i16,
        #[source]
        source: windows::core::Error,
    },

    #[error("pipe read failed: {source}")]
    PipeRead {
        #[source]
        source: windows::core::Error,
    },

    #[error("pipe write failed: {source}")]
    PipeWrite {
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to get console mode: {source}")]
    ConsoleModeGet {
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to set console mode: {source}")]
    ConsoleModeSet {
        #[source]
        source: windows::core::Error,
    },

    #[error("child process exited with code {exit_code}")]
    #[allow(dead_code)] // Phase 2 — used when proxy manages exit codes
    ChildExited { exit_code: u32 },

    #[error("failed to initialize process attribute list: {source}")]
    AttributeList {
        #[source]
        source: windows::core::Error,
    },

    #[error("failed to wait for child process: {source}")]
    WaitFailed {
        #[source]
        source: windows::core::Error,
    },
}

pub type Result<T> = std::result::Result<T, ConPtyError>;
