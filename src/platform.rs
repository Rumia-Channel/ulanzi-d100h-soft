//! The OS layer: process table, foreground app and key/wheel injection.
#[cfg(target_os = "linux")]
pub use dial_linux::{focus_events, running_processes, send_keys, send_wheel};
#[cfg(windows)]
pub use dial_win32::{focus_events, running_processes, send_keys, send_wheel};
