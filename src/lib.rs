// Library entry point — exposes modules for integration tests.

pub mod config;
#[cfg(target_os = "windows")]
pub mod conpty;
pub mod history;
#[cfg(unix)]
pub mod input;
pub mod platform;
pub mod proxy;
pub mod vt;
