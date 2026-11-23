//! /dev/uinput backend: a virtual keyboard and wheel that the compositor
//! treats as real hardware. uinput speaks physical (US-layout) key codes,
//! so characters are emitted as the key that produces them on a US layout,
//! with Shift added as needed.

use std::sync::Mutex;
use std::time::Duration;

use dial_core::{Key, Named};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, InputEvent, KeyCode, KeyEvent, RelativeAxisCode, RelativeAxisEvent};

use super::InjectError;

const NAME: &str = "Ulanzi D100H virtual input";
const UDEV_HINT: &str = "grant access with a udev rule, e.g. \
    KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", TAG+=\"uaccess\"";

/// A freshly created device needs a moment before the compositor/X server
/// registers it and starts routing its events.
const SETTLE: Duration = Duration::from_millis(300);

static DEVICE: Mutex<Option<VirtualDevice>> = Mutex::new(None);
/// Letter keys indexed A–Z. The QWERTY positions are not contiguous in the
/// protocol, so they are tabled rather than computed.
const LETTERS: [KeyCode; 26] = [
    KeyCode::KEY_A,
    KeyCode::KEY_B,
    KeyCode::KEY_C,
    KeyCode::KEY_D,
    KeyCode::KEY_E,
    KeyCode::KEY_F,
    KeyCode::KEY_G,
    KeyCode::KEY_H,
    KeyCode::KEY_I,
    KeyCode::KEY_J,
    KeyCode::KEY_K,
    KeyCode::KEY_L,
    KeyCode::KEY_M,
    KeyCode::KEY_N,
    KeyCode::KEY_O,
    KeyCode::KEY_P,
    KeyCode::KEY_Q,
    KeyCode::KEY_R,
    KeyCode::KEY_S,
    KeyCode::KEY_T,
    KeyCode::KEY_U,
    KeyCode::KEY_V,
    KeyCode::KEY_W,
    KeyCode::KEY_X,
    KeyCode::KEY_Y,
    KeyCode::KEY_Z,
];

/// Main-row digit keys indexed 0–9 (the physical row order is 1–9, 0).
const DIGITS: [KeyCode; 10] = [
    KeyCode::KEY_0,
    KeyCode::KEY_1,
    KeyCode::KEY_2,
    KeyCode::KEY_3,
    KeyCode::KEY_4,
    KeyCode::KEY_5,
    KeyCode::KEY_6,
    KeyCode::KEY_7,
    KeyCode::KEY_8,
    KeyCode::KEY_9,
];

/// Function keys F1–F24.
const FUNCTION: [KeyCode; 24] = [
    KeyCode::KEY_F1,
    KeyCode::KEY_F2,
    KeyCode::KEY_F3,
    KeyCode::KEY_F4,
    KeyCode::KEY_F5,
    KeyCode::KEY_F6,
    KeyCode::KEY_F7,
    KeyCode::KEY_F8,
    KeyCode::KEY_F9,
    KeyCode::KEY_F10,
    KeyCode::KEY_F11,
    KeyCode::KEY_F12,
    KeyCode::KEY_F13,
    KeyCode::KEY_F14,
    KeyCode::KEY_F15,
    KeyCode::KEY_F16,
    KeyCode::KEY_F17,
    KeyCode::KEY_F18,
    KeyCode::KEY_F19,
    KeyCode::KEY_F20,
    KeyCode::KEY_F21,
    KeyCode::KEY_F22,
    KeyCode::KEY_F23,
    KeyCode::KEY_F24,
];

/// Keypad digit keys 0–9.
const KEYPAD: [KeyCode; 10] = [
    KeyCode::KEY_KP0,
    KeyCode::KEY_KP1,
    KeyCode::KEY_KP2,
    KeyCode::KEY_KP3,
    KeyCode::KEY_KP4,
    KeyCode::KEY_KP5,
    KeyCode::KEY_KP6,
    KeyCode::KEY_KP7,
    KeyCode::KEY_KP8,
    KeyCode::KEY_KP9,
];

/// Every fixed key the tables below can emit: modifiers, navigation, the OEM
/// positions named in config (US layout) and media keys.
const FIXED: &[KeyCode] = &[
    KeyCode::KEY_LEFTCTRL,
    KeyCode::KEY_RIGHTCTRL,
    KeyCode::KEY_LEFTALT,
    KeyCode::KEY_RIGHTALT,
    KeyCode::KEY_LEFTSHIFT,
    KeyCode::KEY_RIGHTSHIFT,
    KeyCode::KEY_LEFTMETA,
    KeyCode::KEY_RIGHTMETA,
    KeyCode::KEY_SPACE,
    KeyCode::KEY_ENTER,
    KeyCode::KEY_TAB,
    KeyCode::KEY_ESC,
    KeyCode::KEY_BACKSPACE,
    KeyCode::KEY_DELETE,
    KeyCode::KEY_INSERT,
    KeyCode::KEY_HOME,
    KeyCode::KEY_END,
    KeyCode::KEY_PAGEUP,
    KeyCode::KEY_PAGEDOWN,
    KeyCode::KEY_UP,
    KeyCode::KEY_DOWN,
    KeyCode::KEY_LEFT,
    KeyCode::KEY_RIGHT,
    KeyCode::KEY_EQUAL,
    KeyCode::KEY_MINUS,
    KeyCode::KEY_COMMA,
    KeyCode::KEY_DOT,
    KeyCode::KEY_SLASH,
    KeyCode::KEY_SEMICOLON,
    KeyCode::KEY_APOSTROPHE,
    KeyCode::KEY_BACKSLASH,
    KeyCode::KEY_LEFTBRACE,
    KeyCode::KEY_RIGHTBRACE,
    KeyCode::KEY_GRAVE,
    KeyCode::KEY_KPPLUS,
    KeyCode::KEY_KPMINUS,
    KeyCode::KEY_KPASTERISK,
    KeyCode::KEY_KPSLASH,
    KeyCode::KEY_KPDOT,
    KeyCode::KEY_KPENTER,
    KeyCode::KEY_PLAYPAUSE,
    KeyCode::KEY_NEXTSONG,
    KeyCode::KEY_PREVIOUSSONG,
    KeyCode::KEY_STOPCD,
    KeyCode::KEY_VOLUMEUP,
    KeyCode::KEY_VOLUMEDOWN,
    KeyCode::KEY_MUTE,
];

/// Key capability bits for the virtual device: every code the mapping tables
/// can produce, plus `BTN_LEFT`. Together with REL_X/REL_Y this makes
/// libinput classify the device as a pointer, so wheel events are routed to
/// the focused window instead of being ignored.
fn keyset() -> AttributeSet<KeyCode> {
    let mut keys = AttributeSet::new();
    keys.insert(KeyCode::BTN_LEFT);
    for group in [&LETTERS[..], &DIGITS[..], &FUNCTION[..], &KEYPAD[..]] {
        for &code in group {
            keys.insert(code);
        }
    }
    for &code in FIXED {
        keys.insert(code);
    }
    keys
}

/// Create the virtual keyboard+wheel device. Failing here is almost always a
/// permission problem, so it is reported as [`InjectError::Unavailable`].
fn open() -> Result<VirtualDevice, InjectError> {
    let unavailable = |e: &dyn std::fmt::Display| {
        InjectError::Unavailable(format!("/dev/uinput: {e}; {UDEV_HINT}"))
    };
    let keys = keyset();
    let mut axes = AttributeSet::new();
    for axis in [
        RelativeAxisCode::REL_X,
        RelativeAxisCode::REL_Y,
        RelativeAxisCode::REL_WHEEL,
        RelativeAxisCode::REL_HWHEEL,
        RelativeAxisCode::REL_WHEEL_HI_RES,
        RelativeAxisCode::REL_HWHEEL_HI_RES,
    ] {
        axes.insert(axis);
    }
    VirtualDevice::builder()
        .map_err(|e| unavailable(&e))?
        .name(NAME)
        .with_keys(&keys)
        .map_err(|e| unavailable(&e))?
        .with_relative_axes(&axes)
        .map_err(|e| unavailable(&e))?
        .build()
        .map_err(|e| unavailable(&e))
}

/// Key press (1) / release (0) event for `code`.
fn key_event(code: KeyCode, press: bool) -> InputEvent {
    InputEvent::from(KeyEvent::new(code, i32::from(press)))
}

/// Relative-axis event (e.g. a wheel notch).
fn axis_event(axis: RelativeAxisCode, value: i32) -> InputEvent {
    InputEvent::from(RelativeAxisEvent::new(axis, value))
}

/// evdev key code for a named key; `None` only for out-of-range values
/// (`F(25)`, `Numpad(10)`, non-ASCII `Alnum`).
pub(super) fn named_code(key: Named) -> Option<KeyCode> {
    Some(match key {
        Named::Ctrl => KeyCode::KEY_LEFTCTRL,
        Named::RCtrl => KeyCode::KEY_RIGHTCTRL,
        Named::Alt => KeyCode::KEY_LEFTALT,
        Named::RAlt => KeyCode::KEY_RIGHTALT,
        Named::Shift => KeyCode::KEY_LEFTSHIFT,
        Named::RShift => KeyCode::KEY_RIGHTSHIFT,
        Named::Win => KeyCode::KEY_LEFTMETA,
        Named::RWin => KeyCode::KEY_RIGHTMETA,
        Named::Space => KeyCode::KEY_SPACE,
        Named::Enter => KeyCode::KEY_ENTER,
        Named::Tab => KeyCode::KEY_TAB,
        Named::Esc => KeyCode::KEY_ESC,
        Named::Backspace => KeyCode::KEY_BACKSPACE,
        Named::Delete => KeyCode::KEY_DELETE,
        Named::Insert => KeyCode::KEY_INSERT,
        Named::Home => KeyCode::KEY_HOME,
        Named::End => KeyCode::KEY_END,
        Named::PageUp => KeyCode::KEY_PAGEUP,
        Named::PageDown => KeyCode::KEY_PAGEDOWN,
        Named::Up => KeyCode::KEY_UP,
        Named::Down => KeyCode::KEY_DOWN,
        Named::Left => KeyCode::KEY_LEFT,
        Named::Right => KeyCode::KEY_RIGHT,
        Named::Alnum(c) => match c.to_ascii_uppercase() {
            b @ 'A'..='Z' => LETTERS[(b as u8 - b'A') as usize],
            d @ '0'..='9' => DIGITS[(d as u8 - b'0') as usize],
            _ => return None,
        },
        Named::F(n @ 1..=24) => FUNCTION[usize::from(n) - 1],
        Named::Numpad(n @ 0..=9) => KEYPAD[usize::from(n)],
        Named::NumpadAdd => KeyCode::KEY_KPPLUS,
        Named::NumpadSubtract => KeyCode::KEY_KPMINUS,
        Named::NumpadMultiply => KeyCode::KEY_KPASTERISK,
        Named::NumpadDivide => KeyCode::KEY_KPSLASH,
        Named::NumpadDecimal => KeyCode::KEY_KPDOT,
        Named::NumpadEnter => KeyCode::KEY_KPENTER,
        Named::Equal => KeyCode::KEY_EQUAL,
        Named::Minus => KeyCode::KEY_MINUS,
        Named::Comma => KeyCode::KEY_COMMA,
        Named::Period => KeyCode::KEY_DOT,
        Named::Slash => KeyCode::KEY_SLASH,
        Named::Semicolon => KeyCode::KEY_SEMICOLON,
        Named::Quote => KeyCode::KEY_APOSTROPHE,
        Named::Backslash => KeyCode::KEY_BACKSLASH,
        Named::LBracket => KeyCode::KEY_LEFTBRACE,
        Named::RBracket => KeyCode::KEY_RIGHTBRACE,
        Named::Backquote => KeyCode::KEY_GRAVE,
        Named::PlayPause => KeyCode::KEY_PLAYPAUSE,
        Named::NextTrack => KeyCode::KEY_NEXTSONG,
        Named::PrevTrack => KeyCode::KEY_PREVIOUSSONG,
        Named::MediaStop => KeyCode::KEY_STOPCD,
        Named::VolumeUp => KeyCode::KEY_VOLUMEUP,
        Named::VolumeDown => KeyCode::KEY_VOLUMEDOWN,
        Named::VolumeMute => KeyCode::KEY_MUTE,
        Named::F(_) | Named::Numpad(_) => return None,
    })
}

/// Evdev key code and Shift state for a printable ASCII character on a US
/// layout. uinput events are physical keys, so non-US layouts will see
/// whatever those positions produce instead of the intended character.
fn char_code(c: char) -> Option<(KeyCode, bool)> {
    Some(match c {
        ' ' => (KeyCode::KEY_SPACE, false),
        'a'..='z' => (LETTERS[(c as u8 - b'a') as usize], false),
        'A'..='Z' => (LETTERS[(c as u8 - b'A') as usize], true),
        '0'..='9' => (DIGITS[(c as u8 - b'0') as usize], false),
        '!' => (KeyCode::KEY_1, true),
        '@' => (KeyCode::KEY_2, true),
        '#' => (KeyCode::KEY_3, true),
        '$' => (KeyCode::KEY_4, true),
        '%' => (KeyCode::KEY_5, true),
        '^' => (KeyCode::KEY_6, true),
        '&' => (KeyCode::KEY_7, true),
        '*' => (KeyCode::KEY_8, true),
        '(' => (KeyCode::KEY_9, true),
        ')' => (KeyCode::KEY_0, true),
        '=' | '+' => (KeyCode::KEY_EQUAL, c == '+'),
        '-' | '_' => (KeyCode::KEY_MINUS, c == '_'),
        ',' | '<' => (KeyCode::KEY_COMMA, c == '<'),
        '.' | '>' => (KeyCode::KEY_DOT, c == '>'),
        '/' | '?' => (KeyCode::KEY_SLASH, c == '?'),
        ';' | ':' => (KeyCode::KEY_SEMICOLON, c == ':'),
        '\'' | '"' => (KeyCode::KEY_APOSTROPHE, c == '"'),
        '`' | '~' => (KeyCode::KEY_GRAVE, c == '~'),
        '[' | '{' => (KeyCode::KEY_LEFTBRACE, c == '{'),
        ']' | '}' => (KeyCode::KEY_RIGHTBRACE, c == '}'),
        '\\' | '|' => (KeyCode::KEY_BACKSLASH, c == '|'),
        _ => return None,
    })
}

/// Key name or character, for `InjectError::Unmappable` lists.
fn describe(key: Key) -> String {
    match key {
        Key::Named(named) => format!("{named:?}"),
        Key::Char(c) => c.to_string(),
    }
}

/// Map a whole chord up front; one unmappable key rejects the whole chord
/// without sending anything.
fn resolve(keys: &[Key]) -> Result<Vec<(KeyCode, bool)>, InjectError> {
    let mut codes = Vec::with_capacity(keys.len() + 1);
    let mut missing = Vec::new();
    for &key in keys {
        let mapped = match key {
            Key::Named(named) => named_code(named).map(|code| (code, false)),
            Key::Char(c) => char_code(c),
        };
        match mapped {
            Some(pair) => codes.push(pair),
            None => missing.push(describe(key)),
        }
    }
    if missing.is_empty() {
        Ok(codes)
    } else {
        Err(InjectError::Unmappable(missing))
    }
}

/// Map wheel modifiers the same way.
fn mod_codes(mods: &[Named]) -> Result<Vec<KeyCode>, InjectError> {
    let mut codes = Vec::with_capacity(mods.len());
    let mut missing = Vec::new();
    for &named in mods {
        match named_code(named) {
            Some(code) => codes.push(code),
            None => missing.push(format!("{named:?}")),
        }
    }
    if missing.is_empty() {
        Ok(codes)
    } else {
        Err(InjectError::Unmappable(missing))
    }
}

/// Reports pressing then releasing a chord, one report per key change. A key
/// that needs Shift gets KEY_LEFTSHIFT pressed immediately before it, once,
/// unless the chord already holds Shift. Releases come in the reverse order
/// of pressing, including an auto-added shift. Each report is written with a
/// SYN_REPORT separator by [`VirtualDevice::emit`].
fn chord_reports(keys: &[(KeyCode, bool)], mut has_shift: bool) -> Vec<Vec<InputEvent>> {
    let mut pressed = Vec::with_capacity(keys.len() + 1);
    for &(code, shift) in keys {
        if shift && !has_shift {
            pressed.push(KeyCode::KEY_LEFTSHIFT);
            has_shift = true;
        }
        pressed.push(code);
    }
    pressed
        .iter()
        .map(|&code| vec![key_event(code, true)])
        .chain(
            pressed
                .iter()
                .rev()
                .map(|&code| vec![key_event(code, false)]),
        )
        .collect()
}

/// Reports for scrolling `notches` (positive = up/right) with `mods` held:
/// press the modifiers in order, one report per notch, then release the
/// modifiers in reverse order — even mid-scroll failures leave nothing held.
fn wheel_reports(mods: &[KeyCode], notches: i32, horizontal: bool) -> Vec<Vec<InputEvent>> {
    let (low, high) = if horizontal {
        (
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        )
    } else {
        (
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
        )
    };
    let signum = notches.signum();
    let notches = vec![
        vec![axis_event(low, signum), axis_event(high, signum * 120)];
        notches.unsigned_abs() as usize
    ];
    mods.iter()
        .map(|&code| vec![key_event(code, true)])
        .chain(notches)
        .chain(mods.iter().rev().map(|&code| vec![key_event(code, false)]))
        .collect()
}

fn io(e: impl std::fmt::Display) -> InjectError {
    InjectError::Io(format!("uinput: {e}"))
}

/// Write each report (one `emit` call, so each is terminated by SYN_REPORT).
/// Reports after a failure are still attempted, so the release half of a
/// chord or wheel sequence is never skipped; the first error wins.
fn write_reports(
    device: &mut VirtualDevice,
    reports: &[Vec<InputEvent>],
) -> Result<(), InjectError> {
    let mut result = Ok(());
    for report in reports {
        if let Err(e) = device.emit(report).map_err(io)
            && result.is_ok()
        {
            result = Err(e);
        }
    }
    result
}

/// Run `f` on the shared virtual device, creating it (and letting the
/// compositor register it) on first use. A failed write drops the device so
/// the next call rebuilds a fresh one.
fn with_device(
    f: impl FnOnce(&mut VirtualDevice) -> Result<(), InjectError>,
) -> Result<(), InjectError> {
    let mut guard = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let device = open()?;
        std::thread::sleep(SETTLE);
        *guard = Some(device);
    }
    let result = f(guard.as_mut().expect("device present"));
    if matches!(result, Err(InjectError::Io(_))) {
        // The fd is broken; rebuild on the next call.
        *guard = None;
    }
    result
}

pub(super) fn init() -> Result<(), InjectError> {
    with_device(|_| Ok(()))
}

pub(super) fn send_chord(keys: &[Key]) -> Result<(), InjectError> {
    let codes = resolve(keys)?;
    let has_shift = keys
        .iter()
        .any(|k| matches!(k, Key::Named(Named::Shift | Named::RShift)));
    with_device(|device| write_reports(device, &chord_reports(&codes, has_shift)))
}

pub(super) fn send_wheel(
    mods: &[Named],
    notches: i32,
    horizontal: bool,
) -> Result<(), InjectError> {
    let codes = mod_codes(mods)?;
    with_device(|device| write_reports(device, &wheel_reports(&codes, notches, horizontal)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::EventType;

    /// Every `Named` variant, with the char-coded ones fully expanded.
    fn every_named() -> Vec<Named> {
        let fixed = [
            Named::Ctrl,
            Named::RCtrl,
            Named::Alt,
            Named::RAlt,
            Named::Shift,
            Named::RShift,
            Named::Win,
            Named::RWin,
            Named::Space,
            Named::Enter,
            Named::Tab,
            Named::Esc,
            Named::Backspace,
            Named::Delete,
            Named::Insert,
            Named::Home,
            Named::End,
            Named::PageUp,
            Named::PageDown,
            Named::Up,
            Named::Down,
            Named::Left,
            Named::Right,
            Named::NumpadAdd,
            Named::NumpadSubtract,
            Named::NumpadMultiply,
            Named::NumpadDivide,
            Named::NumpadDecimal,
            Named::NumpadEnter,
            Named::Equal,
            Named::Minus,
            Named::Comma,
            Named::Period,
            Named::Slash,
            Named::Semicolon,
            Named::Quote,
            Named::Backslash,
            Named::LBracket,
            Named::RBracket,
            Named::Backquote,
            Named::PlayPause,
            Named::NextTrack,
            Named::PrevTrack,
            Named::MediaStop,
            Named::VolumeUp,
            Named::VolumeDown,
            Named::VolumeMute,
        ];
        fixed
            .into_iter()
            .chain(('A'..='Z').chain('0'..='9').map(Named::Alnum))
            .chain((1u8..=24).map(Named::F))
            .chain((0u8..=9).map(Named::Numpad))
            .collect()
    }

    /// Compare reports as `(type, code, value)` triples; InputEvent has no Debug.
    fn raw(reports: &[Vec<InputEvent>]) -> Vec<Vec<(u16, u16, i32)>> {
        reports
            .iter()
            .map(|report| {
                report
                    .iter()
                    .map(|e| (e.event_type().0, e.code(), e.value()))
                    .collect()
            })
            .collect()
    }

    fn key_raw(code: KeyCode, value: i32) -> (u16, u16, i32) {
        (EventType::KEY.0, code.0, value)
    }

    fn axis_raw(axis: RelativeAxisCode, value: i32) -> (u16, u16, i32) {
        (EventType::RELATIVE.0, axis.0, value)
    }

    #[test]
    fn every_named_variant_maps_into_capabilities() {
        let keys = keyset();
        for named in every_named() {
            let Some(code) = named_code(named) else {
                panic!("{named:?} has no code")
            };
            assert!(
                keys.contains(code),
                "{named:?} missing from device capabilities"
            );
        }
        // Out-of-range char-coded variants stay unmappable.
        assert_eq!(named_code(Named::Alnum('ä')), None);
        assert_eq!(named_code(Named::F(0)), None);
        assert_eq!(named_code(Named::F(25)), None);
        assert_eq!(named_code(Named::Numpad(10)), None);
    }

    #[test]
    fn us_layout_characters() {
        assert_eq!(char_code('='), Some((KeyCode::KEY_EQUAL, false)));
        assert_eq!(char_code('+'), Some((KeyCode::KEY_EQUAL, true)));
        assert_eq!(char_code('@'), Some((KeyCode::KEY_2, true)));
        assert_eq!(char_code('A'), Some((KeyCode::KEY_A, true)));
        assert_eq!(char_code(' '), Some((KeyCode::KEY_SPACE, false)));
        assert_eq!(char_code('é'), None);
        // Every printable ASCII character maps to a device capability.
        let keys = keyset();
        for c in (0x20u8..=0x7e).map(char::from) {
            let Some((code, _)) = char_code(c) else {
                panic!("{c:?} unmappable")
            };
            assert!(
                keys.contains(code),
                "{c:?} missing from device capabilities"
            );
        }
    }

    #[test]
    fn chord_rejects_whole_on_unmappable() {
        let err = match resolve(&[Key::Named(Named::Ctrl), Key::Char('é')]) {
            Err(InjectError::Unmappable(names)) => names,
            other => panic!("unexpected result: {other:?}"),
        };
        assert_eq!(err, vec!["é".to_string()]);
    }

    #[test]
    fn chord_auto_adds_shift_and_releases_in_reverse() {
        // Ctrl + "+": Shift is inserted before the shifted key and everything
        // comes back out in reverse.
        let reports = chord_reports(
            &[(KeyCode::KEY_LEFTCTRL, false), (KeyCode::KEY_EQUAL, true)],
            false,
        );
        assert_eq!(
            raw(&reports),
            vec![
                vec![key_raw(KeyCode::KEY_LEFTCTRL, 1)],
                vec![key_raw(KeyCode::KEY_LEFTSHIFT, 1)],
                vec![key_raw(KeyCode::KEY_EQUAL, 1)],
                vec![key_raw(KeyCode::KEY_EQUAL, 0)],
                vec![key_raw(KeyCode::KEY_LEFTSHIFT, 0)],
                vec![key_raw(KeyCode::KEY_LEFTCTRL, 0)],
            ]
        );
        // An explicit Shift in the chord means no silent extra press for "+"
        // or a second shifted key.
        let reports = chord_reports(
            &[
                (KeyCode::KEY_LEFTSHIFT, false),
                (KeyCode::KEY_EQUAL, true),
                (KeyCode::KEY_1, true),
            ],
            true,
        );
        assert_eq!(
            raw(&reports),
            vec![
                vec![key_raw(KeyCode::KEY_LEFTSHIFT, 1)],
                vec![key_raw(KeyCode::KEY_EQUAL, 1)],
                vec![key_raw(KeyCode::KEY_1, 1)],
                vec![key_raw(KeyCode::KEY_1, 0)],
                vec![key_raw(KeyCode::KEY_EQUAL, 0)],
                vec![key_raw(KeyCode::KEY_LEFTSHIFT, 0)],
            ]
        );
    }

    #[test]
    fn wheel_alt_two_down_notches() {
        let reports = wheel_reports(&[KeyCode::KEY_LEFTALT], -2, false);
        let notch = || {
            vec![
                axis_raw(RelativeAxisCode::REL_WHEEL, -1),
                axis_raw(RelativeAxisCode::REL_WHEEL_HI_RES, -120),
            ]
        };
        assert_eq!(
            raw(&reports),
            vec![
                vec![key_raw(KeyCode::KEY_LEFTALT, 1)],
                notch(),
                notch(),
                vec![key_raw(KeyCode::KEY_LEFTALT, 0)],
            ]
        );
    }

    #[test]
    fn wheel_one_report_per_horizontal_notch() {
        let reports = wheel_reports(&[], 1, true);
        assert_eq!(
            raw(&reports),
            vec![vec![
                axis_raw(RelativeAxisCode::REL_HWHEEL, 1),
                axis_raw(RelativeAxisCode::REL_HWHEEL_HI_RES, 120)
            ]]
        );
    }
}
