//! Windows helpers for the dial engine: process table, foreground app
//! detection (poll + WinEventHook), and key injection via SendInput.

pub mod focus;
pub mod input;
pub mod list;

pub use focus::{focus_events, foreground_process, process_name_for_window};
pub use input::send_keys;
pub use list::running_processes;
