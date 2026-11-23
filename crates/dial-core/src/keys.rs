//! Platform-neutral key names used in key-chord and wheel-modifier configs.
//! Platform layers map `Key` to their own codes (Win32 VK, Linux evdev).

/// A key named in config. `Char` is a single printable character that the
/// platform resolves through the active keyboard layout (so "=" or "@" means
/// whatever key produces that character, adding Shift etc. as needed).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Named(Named),
    Char(char),
}

/// Keys with a fixed physical meaning. OEM names follow the US layout
/// (e.g. `Equal` is the key right of `0`, whatever it prints elsewhere).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Named {
    Ctrl,
    RCtrl,
    Alt,
    RAlt,
    Shift,
    RShift,
    Win,
    RWin,
    Space,
    Enter,
    Tab,
    Esc,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// Letter A–Z (uppercase) or digit 0–9 on the main row.
    Alnum(char),
    /// F1–F24.
    F(u8),
    /// Numpad 0–9.
    Numpad(u8),
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    Equal,
    Minus,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    Backslash,
    LBracket,
    RBracket,
    Backquote,
    PlayPause,
    NextTrack,
    PrevTrack,
    MediaStop,
    VolumeUp,
    VolumeDown,
    VolumeMute,
}

impl Named {
    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::Ctrl | Self::RCtrl | Self::Alt | Self::RAlt | Self::Shift | Self::RShift | Self::Win | Self::RWin
        )
    }
}

/// Parse one config key name (case-insensitive, surrounding spaces ignored).
pub fn parse_key(name: &str) -> Option<Key> {
    let trimmed = name.trim();
    let n = trimmed.to_lowercase();
    let named = match n.as_str() {
        "ctrl" | "control" | "lctrl" => Named::Ctrl,
        "rctrl" => Named::RCtrl,
        "alt" | "lalt" => Named::Alt,
        "ralt" | "altgr" => Named::RAlt,
        "shift" | "lshift" => Named::Shift,
        "rshift" => Named::RShift,
        "win" | "lwin" | "super" | "meta" | "cmd" => Named::Win,
        "rwin" | "rsuper" => Named::RWin,
        "space" => Named::Space,
        "enter" | "return" => Named::Enter,
        "tab" => Named::Tab,
        "esc" | "escape" => Named::Esc,
        "backspace" => Named::Backspace,
        "delete" | "del" => Named::Delete,
        "insert" | "ins" => Named::Insert,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" | "pgup" => Named::PageUp,
        "pagedown" | "pgdn" => Named::PageDown,
        "up" | "arrowup" => Named::Up,
        "down" | "arrowdown" => Named::Down,
        "left" | "arrowleft" => Named::Left,
        "right" | "arrowright" => Named::Right,
        "plus" | "equal" | "equals" => Named::Equal,
        "minus" => Named::Minus,
        "comma" => Named::Comma,
        "period" | "dot" => Named::Period,
        "slash" => Named::Slash,
        "semicolon" => Named::Semicolon,
        "quote" | "apostrophe" => Named::Quote,
        "backslash" => Named::Backslash,
        "lbracket" => Named::LBracket,
        "rbracket" => Named::RBracket,
        "backquote" | "grave" => Named::Backquote,
        "numadd" | "numplus" | "add" => Named::NumpadAdd,
        "numsubtract" | "numminus" | "subtract" => Named::NumpadSubtract,
        "nummultiply" | "multiply" => Named::NumpadMultiply,
        "numdivide" | "divide" => Named::NumpadDivide,
        "numdecimal" | "decimal" => Named::NumpadDecimal,
        "numenter" => Named::NumpadEnter,
        "play_pause" | "mediaplaypause" => Named::PlayPause,
        "next_track" | "medianext" => Named::NextTrack,
        "prev_track" | "mediaprev" => Named::PrevTrack,
        "stop" | "mediastop" => Named::MediaStop,
        "volume_up" => Named::VolumeUp,
        "volume_down" => Named::VolumeDown,
        "volume_mute" | "mute" => Named::VolumeMute,
        _ => {
            if let Some(f) = n.strip_prefix('f').and_then(|s| s.parse::<u8>().ok())
                && (1..=24).contains(&f)
            {
                return Some(Key::Named(Named::F(f)));
            }
            if let Some(d) = n.strip_prefix("num").and_then(|s| s.parse::<u8>().ok())
                && d <= 9
            {
                return Some(Key::Named(Named::Numpad(d)));
            }
            let mut chars = trimmed.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if c.is_ascii_alphanumeric() {
                return Some(Key::Named(Named::Alnum(c.to_ascii_uppercase())));
            }
            if c.is_control() || c.is_whitespace() {
                return None;
            }
            return Some(Key::Char(c));
        }
    };
    Some(Key::Named(named))
}

/// Parse a chord; returns the names that are not valid keys on failure.
pub fn parse_chord(names: &[String]) -> Result<Vec<Key>, Vec<String>> {
    let mut keys = Vec::with_capacity(names.len());
    let mut unknown = Vec::new();
    for name in names {
        match parse_key(name) {
            Some(key) => keys.push(key),
            None => unknown.push(name.clone()),
        }
    }
    if unknown.is_empty() { Ok(keys) } else { Err(unknown) }
}

/// Parse wheel modifiers; every entry must be a modifier key.
pub fn parse_modifiers(names: &[String]) -> Result<Vec<Named>, Vec<String>> {
    let mut mods = Vec::with_capacity(names.len());
    let mut bad = Vec::new();
    for name in names {
        match parse_key(name) {
            Some(Key::Named(m)) if m.is_modifier() => mods.push(m),
            _ => bad.push(name.clone()),
        }
    }
    if bad.is_empty() { Ok(mods) } else { Err(bad) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn names_and_chars() {
        assert_eq!(parse_key("Ctrl"), Some(Key::Named(Named::Ctrl)));
        assert_eq!(parse_key(" f5 "), Some(Key::Named(Named::F(5))));
        assert_eq!(parse_key("j"), Some(Key::Named(Named::Alnum('J'))));
        assert_eq!(parse_key("7"), Some(Key::Named(Named::Alnum('7'))));
        assert_eq!(parse_key("num7"), Some(Key::Named(Named::Numpad(7))));
        assert_eq!(parse_key("plus"), Some(Key::Named(Named::Equal)));
        assert_eq!(parse_key("="), Some(Key::Char('=')));
        assert_eq!(parse_key("+"), Some(Key::Char('+')));
        assert_eq!(parse_key("@"), Some(Key::Char('@')));
        assert_eq!(parse_key("ArrowLeft"), Some(Key::Named(Named::Left)));
        assert_eq!(parse_key("f25"), None);
        assert_eq!(parse_key("bogus"), None);
        assert_eq!(parse_key(""), None);
    }

    #[test]
    fn chord_rejects_whole_on_unknown() {
        assert_eq!(parse_chord(&s(&["ctrl", "bogus"])), Err(s(&["bogus"])));
        assert_eq!(
            parse_chord(&s(&["ctrl", "="])),
            Ok(vec![Key::Named(Named::Ctrl), Key::Char('=')])
        );
    }

    #[test]
    fn modifiers_only() {
        assert_eq!(parse_modifiers(&s(&["alt", "shift"])), Ok(vec![Named::Alt, Named::Shift]));
        assert_eq!(parse_modifiers(&s(&["alt", "j"])), Err(s(&["j"])));
    }
}
