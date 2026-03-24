// Input handling module
//
// Contains Unix-specific input forwarding, signal handling, and shutdown
// coordination. Only compiled on Unix targets.

#[cfg(unix)]
pub mod unix;
