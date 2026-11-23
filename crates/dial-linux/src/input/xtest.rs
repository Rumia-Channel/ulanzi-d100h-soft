//! XTEST backend: fake key/button events on the X server. Keys resolve
//! through the server's core keymap in the active XKB group, so characters
//! follow the current layout.
use std::sync::Mutex;

use dial_core::{Key, Named};
use x11rb::{
    CURRENT_TIME,
    connection::{Connection, RequestConnection},
    protocol::{
        xkb::{self, ConnectionExt as _},
        xproto::{
            BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt, KEY_PRESS_EVENT,
            KEY_RELEASE_EVENT, Window,
        },
        xtest::{self, ConnectionExt as _},
    },
    rust_connection::RustConnection,
};

use xkeysym::Keysym;

use super::InjectError;

const SHIFT_L: u32 = 0xffe1;
const ISO_LEVEL3_SHIFT: u32 = 0xfe03;

struct Xtest {
    conn: RustConnection,
    root: Window,
    xkb: bool,
}

static CONN: Mutex<Option<Xtest>> = Mutex::new(None);

impl Xtest {
    fn connect() -> Result<Self, InjectError> {
        let unavailable = |e: &dyn std::fmt::Display| InjectError::Unavailable(format!("X11: {e}"));
        let (conn, screen) = x11rb::connect(None).map_err(|e| unavailable(&e))?;
        if conn
            .extension_information(xtest::X11_EXTENSION_NAME)
            .map_err(|e| unavailable(&e))?
            .is_none()
        {
            return Err(InjectError::Unavailable(
                "X server has no XTEST extension".into(),
            ));
        }
        conn.xtest_get_version(2, 2)
            .map_err(|e| unavailable(&e))?
            .reply()
            .map_err(|e| unavailable(&e))?;
        let root = conn.setup().roots[screen].root;
        let xkb = conn
            .xkb_use_extension(1, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.supported);
        Ok(Self { conn, root, xkb })
    }

    /// Active XKB group (0 = first layout); 0 without XKB.
    fn group(&self) -> Result<usize, InjectError> {
        if !self.xkb {
            return Ok(0);
        }
        let state = self
            .conn
            .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
            .map_err(io)?
            .reply()
            .map_err(io)?;
        Ok(usize::from(u8::from(state.group)))
    }

    /// Core keymap plus the active group to resolve keysyms in.
    fn keymap(&self) -> Result<Keymap, InjectError> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let reply = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(io)?
            .reply()
            .map_err(io)?;
        Ok(Keymap {
            min,
            per: usize::from(reply.keysyms_per_keycode),
            syms: reply.keysyms,
            group: self.group()?,
        })
    }

    fn fake(&self, type_: u8, detail: u8) -> Result<(), InjectError> {
        self.conn
            .xtest_fake_input(type_, detail, CURRENT_TIME, self.root, 0, 0, 0)
            .map_err(io)?;
        Ok(())
    }

    /// Round-trip so failures (e.g. a dropped connection) surface here.
    fn sync(&self) -> Result<(), InjectError> {
        self.conn
            .get_input_focus()
            .map_err(io)?
            .reply()
            .map_err(io)?;
        Ok(())
    }

    /// Press `codes` in order, run `between`, release in reverse. Releases are
    /// attempted even if pressing or `between` failed, so nothing stays held.
    fn hold(
        &self,
        codes: &[u8],
        between: impl FnOnce() -> Result<(), InjectError>,
    ) -> Result<(), InjectError> {
        let mut pressed = 0;
        let mut result = Ok(());
        for &code in codes {
            if let Err(e) = self.fake(KEY_PRESS_EVENT, code) {
                result = Err(e);
                break;
            }
            pressed += 1;
        }
        if result.is_ok() {
            result = between();
        }
        for &code in codes[..pressed].iter().rev() {
            let released = self.fake(KEY_RELEASE_EVENT, code);
            if result.is_ok() {
                result = released;
            }
        }
        result.and_then(|()| self.sync())
    }
}

fn io(e: impl std::fmt::Display) -> InjectError {
    InjectError::Io(format!("X11: {e}"))
}

struct Keymap {
    min: u8,
    per: usize,
    syms: Vec<u32>,
    group: usize,
}

/// Core-keymap columns of shift levels 1–4 (plain, Shift, AltGr,
/// AltGr+Shift) per XKB group. Groups past the second are not in the core
/// map; those fall back to the first group.
const LEVEL_COLUMNS: [[usize; 4]; 2] = [[0, 1, 4, 5], [2, 3, 6, 7]];

impl Keymap {
    /// First keycode with a keysym accepted by `wanted` in `group`, as
    /// (keycode, level index into [`LEVEL_COLUMNS`]); lower levels win.
    fn find_in(&self, group: usize, wanted: impl Fn(u32) -> bool) -> Option<(u8, usize)> {
        if self.per == 0 {
            return None;
        }
        let columns = LEVEL_COLUMNS.get(group).unwrap_or(&LEVEL_COLUMNS[0]);
        columns.iter().enumerate().find_map(|(level, &col)| {
            self.syms
                .chunks(self.per)
                .zip(self.min..=u8::MAX)
                .find(|(row, _)| row.get(col).is_some_and(|&sym| sym != 0 && wanted(sym)))
                .map(|(_, code)| (code, level))
        })
    }

    /// Named keys: the active group first, then any other, since for
    /// shortcuts like Ctrl+C the physical key is what matters.
    fn find_named(&self, keysym: u32) -> Option<(u8, usize)> {
        std::iter::once(self.group)
            .chain(0..LEVEL_COLUMNS.len())
            .find_map(|group| self.find_in(group, |sym| sym == keysym))
    }

    /// Characters: the active group only — another group's key would type
    /// something else. Legacy keysyms (e.g. Cyrillic_shorti for `й`) match
    /// by the character they produce; keypad keys never stand in for it.
    fn find_char(&self, c: char) -> Option<(u8, usize)> {
        let exact = char_keysym(c);
        self.find_in(self.group, |sym| sym == exact).or_else(|| {
            self.find_in(self.group, |sym| {
                let sym = Keysym::new(sym);
                !sym.is_keypad_key() && sym.key_char() == Some(c)
            })
        })
    }
}

/// X keycode for an evdev scancode, for keys whose physical position matters.
/// Holds for every evdev/libinput-based X server, including XWayland.
const fn evdev(code: u8) -> Sym {
    Sym::Code(code + 8)
}

enum Sym {
    Keysym(u32),
    Code(u8),
}

fn named_sym(key: Named) -> Option<Sym> {
    use Sym::Keysym as K;
    Some(match key {
        Named::Ctrl => K(0xffe3),
        Named::RCtrl => K(0xffe4),
        Named::Alt => K(0xffe9),
        Named::RAlt => evdev(100),
        Named::Shift => K(SHIFT_L),
        Named::RShift => K(0xffe2),
        Named::Win => K(0xffeb),
        Named::RWin => K(0xffec),
        Named::Space => K(0x20),
        Named::Enter => K(0xff0d),
        Named::Tab => K(0xff09),
        Named::Esc => K(0xff1b),
        Named::Backspace => K(0xff08),
        Named::Delete => K(0xffff),
        Named::Insert => K(0xff63),
        Named::Home => K(0xff50),
        Named::End => K(0xff57),
        Named::PageUp => K(0xff55),
        Named::PageDown => K(0xff56),
        Named::Left => K(0xff51),
        Named::Up => K(0xff52),
        Named::Right => K(0xff53),
        Named::Down => K(0xff54),
        Named::Alnum(c) if c.is_ascii_alphanumeric() => K(u32::from(c.to_ascii_lowercase())),
        Named::F(n @ 1..=24) => K(0xffbd + u32::from(n)),
        // Physical numpad keys; what they type depends on NumLock as usual.
        Named::Numpad(n @ 0..=9) => evdev([82, 79, 80, 81, 75, 76, 77, 71, 72, 73][usize::from(n)]),
        Named::NumpadMultiply => evdev(55),
        Named::NumpadSubtract => evdev(74),
        Named::NumpadAdd => evdev(78),
        Named::NumpadDecimal => evdev(83),
        Named::NumpadEnter => evdev(96),
        Named::NumpadDivide => evdev(98),
        Named::Minus => evdev(12),
        Named::Equal => evdev(13),
        Named::LBracket => evdev(26),
        Named::RBracket => evdev(27),
        Named::Semicolon => evdev(39),
        Named::Quote => evdev(40),
        Named::Backquote => evdev(41),
        Named::Backslash => evdev(43),
        Named::Comma => evdev(51),
        Named::Period => evdev(52),
        Named::Slash => evdev(53),
        Named::VolumeDown => K(0x1008_ff11),
        Named::VolumeMute => K(0x1008_ff12),
        Named::VolumeUp => K(0x1008_ff13),
        Named::PlayPause => K(0x1008_ff14),
        Named::MediaStop => K(0x1008_ff15),
        Named::PrevTrack => K(0x1008_ff16),
        Named::NextTrack => K(0x1008_ff17),
        Named::Alnum(_) | Named::F(_) | Named::Numpad(_) => return None,
    })
}

/// Keysym for a printable character (Latin-1 is identity, rest is Unicode).
fn char_keysym(c: char) -> u32 {
    match u32::from(c) {
        cp @ (0x20..=0x7e | 0xa0..=0xff) => cp,
        cp => 0x0100_0000 | cp,
    }
}

fn describe(key: Key) -> String {
    match key {
        Key::Named(named) => format!("{named:?}"),
        Key::Char(c) => c.to_string(),
    }
}

/// Letters and digits missing from every layout group (e.g. a Russian-only
/// keymap) fall back to their US physical key, like a shortcut typed by hand.
fn physical(key: Named) -> Option<(u8, usize)> {
    match key {
        Named::Alnum(_) => super::uinput::named_code(key)
            .and_then(|code| u8::try_from(code.0 + 8).ok())
            .map(|code| (code, 0)),
        _ => None,
    }
}

/// Add the key for modifier `keysym` unless already pressed.
fn push_modifier(
    keymap: &Keymap,
    codes: &mut Vec<u8>,
    missing: &mut Vec<String>,
    keysym: u32,
    name: &str,
) {
    match keymap.find_named(keysym) {
        Some((code, _)) if !codes.contains(&code) => codes.push(code),
        Some(_) => {}
        None => missing.push(name.to_string()),
    }
}

/// Keycodes to press in order. A key on a shifted level gets Shift pressed
/// before it unless the chord already holds one; level 3/4 keys get AltGr
/// (ISO_Level3_Shift).
fn resolve(keymap: &Keymap, keys: &[Key]) -> Result<Vec<u8>, InjectError> {
    let lookup = |sym: Sym| match sym {
        Sym::Code(code) => Some((code, 0)),
        Sym::Keysym(keysym) => keymap.find_named(keysym),
    };
    let mut codes = Vec::new();
    let mut missing = Vec::new();
    let has_shift = keys
        .iter()
        .any(|k| matches!(k, Key::Named(Named::Shift | Named::RShift)));
    for &key in keys {
        let found = match key {
            Key::Named(named) => named_sym(named)
                .and_then(lookup)
                .or_else(|| physical(named)),
            Key::Char(c) => keymap.find_char(c),
        };
        match found {
            Some((code, level)) => {
                if level % 2 == 1 && !has_shift {
                    push_modifier(keymap, &mut codes, &mut missing, SHIFT_L, "Shift");
                }
                if level >= 2 {
                    push_modifier(keymap, &mut codes, &mut missing, ISO_LEVEL3_SHIFT, "AltGr");
                }
                codes.push(code);
            }
            None => missing.push(describe(key)),
        }
    }
    if missing.is_empty() {
        Ok(codes)
    } else {
        Err(InjectError::Unmappable(missing))
    }
}

/// Run `f` on the shared connection, reconnecting once if it was dropped.
fn with_conn<T>(f: impl Fn(&Xtest) -> Result<T, InjectError>) -> Result<T, InjectError> {
    let mut guard = CONN.lock().unwrap_or_else(|e| e.into_inner());
    for attempt in 0..2 {
        if guard.is_none() {
            *guard = Some(Xtest::connect()?);
        }
        match f(guard.as_ref().expect("connected")) {
            Err(InjectError::Io(_)) if attempt == 0 => *guard = None,
            other => return other,
        }
    }
    unreachable!()
}

pub(super) fn init() -> Result<(), InjectError> {
    with_conn(|_| Ok(()))
}

pub(super) fn send_chord(keys: &[Key]) -> Result<(), InjectError> {
    with_conn(|x| {
        let codes = resolve(&x.keymap()?, keys)?;
        x.hold(&codes, || Ok(()))
    })
}

pub(super) fn send_wheel(
    mods: &[Named],
    notches: i32,
    horizontal: bool,
) -> Result<(), InjectError> {
    // Core buttons 4/5 scroll up/down, 6/7 left/right.
    let button = match (horizontal, notches > 0) {
        (false, true) => 4,
        (false, false) => 5,
        (true, false) => 6,
        (true, true) => 7,
    };
    let keys: Vec<Key> = mods.iter().copied().map(Key::Named).collect();
    with_conn(|x| {
        let codes = resolve(&x.keymap()?, &keys)?;
        x.hold(&codes, || {
            for _ in 0..notches.unsigned_abs() {
                x.fake(BUTTON_PRESS_EVENT, button)?;
                x.fake(BUTTON_RELEASE_EVENT, button)?;
            }
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PER: usize = 8;

    /// Tiny keymap, group 1 US-like / group 2 Russian-like: 10 = 1/!,
    /// 21 = =/+, 24 = q/Q | й/Й (AltGr: @), 38 = a/A | ф/Ф, 50 = Shift_L,
    /// 37 = Control_L, 92 = ISO_Level3_Shift, 86 = KP_Add.
    fn keymap(group: usize) -> Keymap {
        let mut syms = vec![0; 100 * PER];
        let mut set = |code: usize, col: usize, sym: u32| syms[(code - 8) * PER + col] = sym;
        let mut both = |code: usize, plain: u32, shifted: u32| {
            for col in [0, 2] {
                set(code, col, plain);
                set(code, col + 1, shifted);
            }
        };
        both(10, '1' as u32, '!' as u32);
        both(21, '=' as u32, '+' as u32);
        both(50, SHIFT_L, 0);
        both(37, 0xffe3, 0);
        both(92, ISO_LEVEL3_SHIFT, 0);
        both(86, 0xffab, 0);
        for (code, cols) in [
            (24, ['q' as u32, 'Q' as u32, 0x6ca, 0x6ea, '@' as u32]),
            (38, ['a' as u32, 'A' as u32, 0x6c6, 0x6e6, 0]),
        ] {
            for (col, sym) in cols.into_iter().enumerate() {
                set(code, col, sym);
            }
        }
        Keymap {
            min: 8,
            per: PER,
            syms,
            group,
        }
    }

    #[test]
    fn resolves_through_keymap() {
        let km = keymap(0);
        let codes = |keys: &[Key]| resolve(&km, keys).map_err(|e| e.to_string());
        assert_eq!(
            codes(&[Key::Named(Named::Ctrl), Key::Char('=')]),
            Ok(vec![37, 21])
        );
        assert_eq!(
            codes(&[Key::Named(Named::Ctrl), Key::Char('+')]),
            Ok(vec![37, 50, 21])
        );
        assert_eq!(
            codes(&[Key::Named(Named::Shift), Key::Char('!')]),
            Ok(vec![50, 10])
        );
        assert_eq!(codes(&[Key::Named(Named::Alnum('A'))]), Ok(vec![38]));
        assert_eq!(codes(&[Key::Named(Named::Equal)]), Ok(vec![21]));
        assert!(
            codes(&[Key::Named(Named::Ctrl), Key::Char('é')])
                .unwrap_err()
                .contains('é')
        );
        // A keypad key never stands in for the main-row character.
        assert_eq!(codes(&[Key::Char('+')]), Ok(vec![50, 21]));
        assert_eq!(codes(&[Key::Char('@')]), Ok(vec![92, 24]));
        assert!(codes(&[Key::Char('й')]).is_err());
    }

    #[test]
    fn follows_the_active_group() {
        let km = keymap(1);
        let codes = |keys: &[Key]| resolve(&km, keys).map_err(|e| e.to_string());
        // Legacy Cyrillic keysyms match by character.
        assert_eq!(codes(&[Key::Char('й')]), Ok(vec![24]));
        assert_eq!(codes(&[Key::Char('Ф')]), Ok(vec![50, 38]));
        // Latin letters are not in group 2: a character is unmappable, a
        // named key falls back to its group-1 physical key.
        assert!(codes(&[Key::Char('q')]).is_err());
        assert_eq!(
            codes(&[Key::Named(Named::Ctrl), Key::Named(Named::Alnum('Q'))]),
            Ok(vec![37, 24])
        );
        assert_eq!(codes(&[Key::Char('+')]), Ok(vec![50, 21]));
        // Not in the keymap at all: the US physical key (evdev KEY_Z + 8).
        assert_eq!(codes(&[Key::Named(Named::Alnum('Z'))]), Ok(vec![52]));
    }

    #[test]
    fn every_named_key_has_a_symbol() {
        let mut all = vec![
            Named::Ctrl,
            Named::RAlt,
            Named::NumpadEnter,
            Named::VolumeMute,
        ];
        all.extend(('A'..='Z').chain('0'..='9').map(Named::Alnum));
        all.extend((1..=24).map(Named::F));
        all.extend((0..=9).map(Named::Numpad));
        assert!(all.into_iter().all(|k| named_sym(k).is_some()));
    }
}
