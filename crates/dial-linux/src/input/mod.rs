//! Key-chord and mouse-wheel injection.
//!
//! X11 sessions use XTEST: characters resolve through the server keymap, so
//! "=" types whatever key produces "=" on the active layout. Wayland (or no
//! X server) uses a /dev/uinput virtual keyboard+wheel, which the compositor
//! treats as real hardware; characters there assume a US layout.
//! `ULANZI_D100H_INPUT=xtest|uinput` overrides the choice.
mod uinput;
mod xtest;

use std::sync::OnceLock;

use dial_core::{parse_chord, parse_modifiers};

/// Why an action could not be (fully) injected.
#[derive(Debug)]
pub enum InjectError {
    /// Names that are not valid keys (or not modifiers, for wheel).
    UnknownKeys(Vec<String>),
    /// Keys/characters the backend cannot produce (not in the keymap).
    Unmappable(Vec<String>),
    /// The backend cannot be used (no X server, no XTEST, /dev/uinput denied…).
    Unavailable(String),
    /// Writing the events failed.
    Io(String),
}

impl std::fmt::Display for InjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKeys(names) => write!(f, "unknown key name(s): {}", names.join(", ")),
            Self::Unmappable(names) => {
                write!(
                    f,
                    "not typable on the current keyboard layout: {}",
                    names.join(", ")
                )
            }
            Self::Unavailable(why) => write!(f, "input injection unavailable: {why}"),
            Self::Io(why) => write!(f, "input injection failed: {why}"),
        }
    }
}

impl std::error::Error for InjectError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backend {
    XTest,
    Uinput,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::XTest => "XTEST",
            Self::Uinput => "uinput",
        })
    }
}

fn choose(forced: Option<&str>, display: bool, wayland: bool) -> Backend {
    match forced.map(str::to_ascii_lowercase).as_deref() {
        Some("xtest") => Backend::XTest,
        Some("uinput") => Backend::Uinput,
        _ if display && !wayland => Backend::XTest,
        _ => Backend::Uinput,
    }
}

pub fn backend() -> Backend {
    static BACKEND: OnceLock<Backend> = OnceLock::new();
    *BACKEND.get_or_init(|| {
        let set = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        choose(
            std::env::var("ULANZI_D100H_INPUT").ok().as_deref(),
            set("DISPLAY"),
            set("WAYLAND_DISPLAY"),
        )
    })
}

/// Open the backend up front: a fresh uinput device needs a moment before
/// the compositor routes its events, and permission problems surface early.
pub fn init_input() -> Result<Backend, InjectError> {
    let backend = backend();
    match backend {
        Backend::XTest => xtest::init(),
        Backend::Uinput => uinput::init(),
    }
    .map(|()| backend)
}

/// Press `names` in order, then release in reverse. The whole chord is
/// rejected if any name is unknown, so a typo never leaves a lone Ctrl tap.
pub fn send_keys(names: &[String]) -> Result<(), InjectError> {
    let keys = parse_chord(names).map_err(InjectError::UnknownKeys)?;
    if keys.is_empty() {
        return Ok(());
    }
    match backend() {
        Backend::XTest => xtest::send_chord(&keys),
        Backend::Uinput => uinput::send_chord(&keys),
    }
}

/// Scroll `notches` (positive = up / right) with `modifiers` held.
pub fn send_wheel(modifiers: &[String], notches: i32, horizontal: bool) -> Result<(), InjectError> {
    let mods = parse_modifiers(modifiers).map_err(InjectError::UnknownKeys)?;
    if notches == 0 {
        return Ok(());
    }
    match backend() {
        Backend::XTest => xtest::send_wheel(&mods, notches, horizontal),
        Backend::Uinput => uinput::send_wheel(&mods, notches, horizontal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_choice() {
        assert_eq!(choose(None, true, false), Backend::XTest);
        assert_eq!(choose(None, true, true), Backend::Uinput);
        assert_eq!(choose(None, false, false), Backend::Uinput);
        assert_eq!(choose(Some("XTest"), false, true), Backend::XTest);
        assert_eq!(choose(Some("uinput"), true, false), Backend::Uinput);
        assert_eq!(choose(Some("bogus"), true, false), Backend::XTest);
    }
}
