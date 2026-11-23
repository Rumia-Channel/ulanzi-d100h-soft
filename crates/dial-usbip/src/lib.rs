//! Emulated Blackmagic DaVinci Resolve Speed Editor exposed over the
//! usbip-win2 protocol so DaVinci Resolve binds its native driver and
//! treats the dial as a real control surface (jog wheel, transport keys).
//!
//! Protocol knowledge is adapted from KipJM/blackmacro-lib (pico
//! firmware) and smunaut/blackmagic-misc, cross-checked against a real
//! device descriptor dump (abclinuxu.cz lsusb).

pub mod device;
mod server;

pub use device::{SpeedEditor, SpeedEditorKey, SpeedEditorState, key_by_name};
pub use server::Server;
