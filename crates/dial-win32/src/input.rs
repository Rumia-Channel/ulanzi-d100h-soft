//! Key-chord and mouse-wheel injection via SendInput.
use dial_core::{Key, Named, parse_chord, parse_modifiers};
use windows_sys::Win32::{
    Foundation::GetLastError,
    UI::{
        Input::KeyboardAndMouse::{
            GetKeyboardLayout, HKL, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
            KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC_EX, MOUSEEVENTF_HWHEEL,
            MOUSEEVENTF_WHEEL, MOUSEINPUT, MapVirtualKeyExW, SendInput, VkKeyScanExW,
        },
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

const WHEEL_DELTA: i32 = 120;

/// Why an action could not be (fully) injected.
#[derive(Debug)]
pub enum InjectError {
    /// Names that are not valid keys (or not modifiers, for wheel).
    UnknownKeys(Vec<String>),
    /// Characters the active keyboard layout cannot type.
    Unmappable(Vec<String>),
    /// SendInput inserted fewer events than requested. Usually UIPI: the
    /// target runs elevated (as administrator) and this app does not.
    Blocked { sent: u32, expected: u32, error: u32 },
}

impl std::fmt::Display for InjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKeys(names) => write!(f, "unknown key name(s): {}", names.join(", ")),
            Self::Unmappable(names) => {
                write!(f, "not typable on the current keyboard layout: {}", names.join(", "))
            }
            Self::Blocked { sent, expected, error } => write!(
                f,
                "SendInput inserted {sent}/{expected} events (error {error}); \
                 if the target app runs as administrator, run this app elevated too"
            ),
        }
    }
}

impl std::error::Error for InjectError {}

/// Win32 VK for a fixed-meaning key.
fn named_vk(key: Named) -> u16 {
    match key {
        Named::Ctrl => 0xA2,
        Named::RCtrl => 0xA3,
        Named::Alt => 0xA4,
        Named::RAlt => 0xA5,
        Named::Shift => 0xA0,
        Named::RShift => 0xA1,
        Named::Win => 0x5B,
        Named::RWin => 0x5C,
        Named::Space => 0x20,
        Named::Enter | Named::NumpadEnter => 0x0D,
        Named::Tab => 0x09,
        Named::Esc => 0x1B,
        Named::Backspace => 0x08,
        Named::Delete => 0x2E,
        Named::Insert => 0x2D,
        Named::Home => 0x24,
        Named::End => 0x23,
        Named::PageUp => 0x21,
        Named::PageDown => 0x22,
        Named::Up => 0x26,
        Named::Down => 0x28,
        Named::Left => 0x25,
        Named::Right => 0x27,
        Named::Alnum(c) => c as u16,
        Named::F(n) => 0x6F + u16::from(n), // F1=0x70 … F24=0x87
        Named::Numpad(n) => 0x60 + u16::from(n),
        Named::NumpadMultiply => 0x6A,
        Named::NumpadAdd => 0x6B,
        Named::NumpadSubtract => 0x6D,
        Named::NumpadDecimal => 0x6E,
        Named::NumpadDivide => 0x6F,
        Named::Equal => 0xBB,
        Named::Minus => 0xBD,
        Named::Comma => 0xBC,
        Named::Period => 0xBE,
        Named::Slash => 0xBF,
        Named::Semicolon => 0xBA,
        Named::Quote => 0xDE,
        Named::Backslash => 0xDC,
        Named::LBracket => 0xDB,
        Named::RBracket => 0xDD,
        Named::Backquote => 0xC0,
        Named::PlayPause => 0xB3,
        Named::NextTrack => 0xB0,
        Named::PrevTrack => 0xB1,
        Named::MediaStop => 0xB2,
        Named::VolumeUp => 0xAF,
        Named::VolumeDown => 0xAE,
        Named::VolumeMute => 0xAD,
    }
}

/// Keyboard layout of the foreground window's thread — the layout the
/// target app will use to interpret the injected keys.
fn foreground_layout() -> HKL {
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        GetKeyboardLayout(thread)
    }
}

/// A VK plus whether it is the extended (E0) variant of a shared VK, e.g.
/// numpad Enter is VK_RETURN + extended.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stroke {
    vk: u16,
    extended: bool,
}

impl Stroke {
    fn plain(vk: u16) -> Self {
        Self { vk, extended: false }
    }

    fn named(key: Named) -> Self {
        Self { vk: named_vk(key), extended: key == Named::NumpadEnter }
    }
}

/// Strokes to hold for one key: a character may need Shift/Ctrl/Alt on top.
fn key_strokes(key: Key, layout: HKL) -> Option<Vec<Stroke>> {
    match key {
        Key::Named(named) => Some(vec![Stroke::named(named)]),
        Key::Char(c) => {
            let mut buf = [0u16; 2];
            let [unit] = c.encode_utf16(&mut buf) else {
                return None;
            };
            let scan = unsafe { VkKeyScanExW(*unit, layout) };
            if scan == -1 {
                return None;
            }
            let vk = (scan as u16) & 0xFF;
            let shift_state = ((scan as u16) >> 8) & 0xFF;
            let mut strokes = Vec::with_capacity(4);
            if shift_state & 1 != 0 {
                strokes.push(Stroke::named(Named::Shift));
            }
            if shift_state & 2 != 0 {
                strokes.push(Stroke::named(Named::Ctrl));
            }
            if shift_state & 4 != 0 {
                strokes.push(Stroke::named(Named::Alt));
            }
            strokes.push(Stroke::plain(vk));
            Some(strokes)
        }
    }
}

/// Press a key chord: hold every key in order, release in reverse.
/// Nothing is sent unless every name resolves, so a typo never leaves a
/// lone modifier tap behind.
pub fn send_keys(names: &[String]) -> Result<(), InjectError> {
    let keys = parse_chord(names).map_err(InjectError::UnknownKeys)?;
    let layout = foreground_layout();
    let mut strokes: Vec<Stroke> = Vec::with_capacity(keys.len());
    let mut unmappable = Vec::new();
    for (key, name) in keys.into_iter().zip(names) {
        match key_strokes(key, layout) {
            Some(more) => {
                for stroke in more {
                    if !strokes.contains(&stroke) {
                        strokes.push(stroke);
                    }
                }
            }
            None => unmappable.push(name.clone()),
        }
    }
    if !unmappable.is_empty() {
        return Err(InjectError::Unmappable(unmappable));
    }
    if strokes.is_empty() {
        return Ok(());
    }
    let mut inputs: Vec<INPUT> = Vec::with_capacity(strokes.len() * 2);
    inputs.extend(strokes.iter().map(|&s| key_input(s, false, layout)));
    inputs.extend(strokes.iter().rev().map(|&s| key_input(s, true, layout)));
    send(&inputs)
}

/// Scroll `notches` wheel clicks (positive = up / right) with modifiers held.
/// The wheel goes to the window under the mouse cursor, as with a real mouse.
pub fn send_wheel(modifiers: &[String], notches: i32, horizontal: bool) -> Result<(), InjectError> {
    let mods = parse_modifiers(modifiers).map_err(InjectError::UnknownKeys)?;
    if notches == 0 {
        return Ok(());
    }
    let layout = foreground_layout();
    let mut strokes: Vec<Stroke> = Vec::with_capacity(mods.len());
    for stroke in mods.into_iter().map(Stroke::named) {
        if !strokes.contains(&stroke) {
            strokes.push(stroke);
        }
    }
    let mut inputs: Vec<INPUT> = Vec::with_capacity(strokes.len() * 2 + 1);
    inputs.extend(strokes.iter().map(|&s| key_input(s, false, layout)));
    inputs.push(wheel_input(notches.saturating_mul(WHEEL_DELTA), horizontal));
    inputs.extend(strokes.iter().rev().map(|&s| key_input(s, true, layout)));
    send(&inputs)
}

fn send(inputs: &[INPUT]) -> Result<(), InjectError> {
    let expected = inputs.len() as u32;
    let sent = unsafe { SendInput(expected, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if sent == expected {
        Ok(())
    } else {
        Err(InjectError::Blocked { sent, expected, error: unsafe { GetLastError() } })
    }
}

/// VKs that need KEYEVENTF_EXTENDEDKEY even if MapVirtualKey omits the E0 prefix.
fn is_extended(vk: u16) -> bool {
    matches!(vk,
        0xA3 | 0xA5 | 0x5B | 0x5C | 0x2D | 0x2E | 0x21 | 0x22 | 0x23 | 0x24
        | 0x25 | 0x26 | 0x27 | 0x28 | 0x6F | 0xAF | 0xAE | 0xAD | 0xB0 | 0xB1 | 0xB2 | 0xB3)
}

fn key_input(stroke: Stroke, up: bool, layout: HKL) -> INPUT {
    let Stroke { vk, extended } = stroke;
    // Fill the hardware scan code too: apps that read lParam's scan code
    // (or map by physical key) ignore events whose scan code is 0. Map it with
    // the target's layout, the same one characters were resolved against.
    let scan_ex = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, layout) };
    let mut flags = 0u32;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended || is_extended(vk) || matches!(scan_ex >> 8, 0xE0 | 0xE1) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: (scan_ex & 0xFF) as u16,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn wheel_input(delta: i32, horizontal: bool) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: if horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::named_vk;
    use dial_core::Named;

    #[test]
    fn resolves_common_names() {
        assert_eq!(named_vk(Named::Ctrl), 0xA2);
        assert_eq!(named_vk(Named::Shift), 0xA0);
        assert_eq!(named_vk(Named::F(5)), 0x74);
        assert_eq!(named_vk(Named::Alnum('J')), 0x4A);
        assert_eq!(named_vk(Named::Numpad(7)), 0x67);
        assert_eq!(named_vk(Named::VolumeUp), 0xAF);
    }
}
