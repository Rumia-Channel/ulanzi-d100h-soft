//! Key-name → VK resolution and chord injection via SendInput.
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput,
};

/// Translate a config key name to a virtual-key code.
/// Covers modifiers, letters, digits, F-keys, editing keys, and media keys.
fn key_code(name: &str) -> Option<u16> {
    let n = name.trim().to_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" | "lctrl" => 0xA2,
        "rctrl" => 0xA3,
        "alt" | "lalt" => 0xA4,
        "ralt" => 0xA5,
        "shift" | "lshift" => 0xA0,
        "rshift" => 0xA1,
        "win" | "lwin" => 0x5B,
        "rwin" => 0x5C,
        "space" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "plus" | "+" => 0xBB,
        "minus" | "-" => 0xBD,
        "comma" | "," => 0xBC,
        "period" | "." => 0xBE,
        "slash" | "/" => 0xBF,
        "semicolon" | ";" => 0xBA,
        "quote" | "'" => 0xDE,
        "backslash" | "\\" => 0xDC,
        "lbracket" | "[" => 0xDB,
        "rbracket" | "]" => 0xDD,
        "backquote" | "`" => 0xC0,
        // Media keys (extended VKs)
        "play_pause" | "mediaplaypause" => 0xB3,
        "next_track" | "medianext" => 0xB0,
        "prev_track" | "mediaprev" => 0xB1,
        "stop" | "mediastop" => 0xB2,
        "volume_up" => 0xAF,
        "volume_down" => 0xAE,
        "volume_mute" | "mute" => 0xAD,
        _ => {
            if let Some(f) = n.strip_prefix('f').and_then(|s| s.parse::<u16>().ok())
                && (1..=24).contains(&f)
            {
                return Some(0x6F + f); // F1=0x70 … F24=0x87
            }
            if n.len() == 1 {
                let c = n.chars().next().unwrap();
                if c.is_ascii_alphanumeric() {
                    return Some(c.to_ascii_uppercase() as u16);
                }
            }
            return None;
        }
    })
}

/// VKs that need KEYEVENTF_EXTENDEDKEY.
fn is_extended(vk: u16) -> bool {
    matches!(vk,
        0xA3 | 0xA5 | 0x5B | 0x5C | 0x2D | 0x2E | 0x21 | 0x22 | 0x23 | 0x24
        | 0x25 | 0x26 | 0x27 | 0x28 | 0xAF | 0xAE | 0xAD | 0xB0 | 0xB1 | 0xB2 | 0xB3)
}

/// Press a key chord: hold every key in order, release in reverse.
/// Returns the key names that failed to resolve (everything else was sent).
pub fn send_keys(names: &[String]) -> Vec<String> {
    let codes: Vec<u16> = names.iter().filter_map(|n| key_code(n)).collect();
    let rejected: Vec<String> = names
        .iter()
        .filter(|n| key_code(n).is_none())
        .cloned()
        .collect();
    if codes.is_empty() {
        return rejected;
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(codes.len() * 2);
    for &vk in &codes {
        inputs.push(key_input(vk, false));
    }
    for &vk in codes.iter().rev() {
        inputs.push(key_input(vk, true));
    }
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
    rejected
}

fn key_input(vk: u16, up: bool) -> INPUT {
    let mut flags = 0u32;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::key_code;

    #[test]
    fn resolves_common_names() {
        assert_eq!(key_code("ctrl"), Some(0xA2));
        assert_eq!(key_code("Shift"), Some(0xA0));
        assert_eq!(key_code("f5"), Some(0x74));
        assert_eq!(key_code("j"), Some(0x4A));
        assert_eq!(key_code("volume_up"), Some(0xAF));
        assert_eq!(key_code("bogus"), None);
    }
}
