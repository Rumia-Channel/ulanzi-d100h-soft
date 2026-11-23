//! Linux helpers for the dial engine: process table from /proc, foreground
//! app detection via X11 `_NET_ACTIVE_WINDOW`, and key/wheel injection via
//! XTEST (X11 sessions) or a /dev/uinput virtual device (Wayland).

pub mod focus;
pub mod input;
pub mod list;

pub use focus::{focus_events, foreground_process};
pub use input::{Backend, InjectError, init_input, send_keys, send_wheel};
pub use list::running_processes;
