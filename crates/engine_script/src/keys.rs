use engine_platform::KeyCode;

/// Looks a key up by the name scripts use: `"W"`, `"Space"`, `"Left"`,
/// `"1"`, `"Shift"`, `"F5"`. Names are case-insensitive, and winit's own
/// names (`"KeyW"`, `"ArrowLeft"`, `"Digit1"`) work too.
pub(crate) fn key_from_name(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    const FUNCTION: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];

    let lower = name.to_ascii_lowercase();
    let short = lower
        .strip_prefix("key")
        .or_else(|| lower.strip_prefix("digit"))
        .or_else(|| lower.strip_prefix("arrow"))
        .unwrap_or(&lower);

    if let [c] = short.as_bytes() {
        match c {
            b'a'..=b'z' => return Some(LETTERS[(c - b'a') as usize]),
            b'0'..=b'9' => return Some(DIGITS[(c - b'0') as usize]),
            _ => {}
        }
    }
    if let Some(n) = short
        .strip_prefix('f')
        .and_then(|n| n.parse::<usize>().ok())
    {
        return FUNCTION.get(n.checked_sub(1)?).copied();
    }
    Some(match short {
        "space" => Space,
        "enter" | "return" => Enter,
        "escape" | "esc" => Escape,
        "tab" => Tab,
        "backspace" => Backspace,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "shift" | "shiftleft" => ShiftLeft,
        "shiftright" => ShiftRight,
        "ctrl" | "control" | "controlleft" => ControlLeft,
        "controlright" => ControlRight,
        "alt" | "altleft" => AltLeft,
        "altright" => AltRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_long_names_agree() {
        assert_eq!(key_from_name("W"), Some(KeyCode::KeyW));
        assert_eq!(key_from_name("w"), Some(KeyCode::KeyW));
        assert_eq!(key_from_name("KeyW"), Some(KeyCode::KeyW));
        assert_eq!(key_from_name("7"), Some(KeyCode::Digit7));
        assert_eq!(key_from_name("Digit7"), Some(KeyCode::Digit7));
        assert_eq!(key_from_name("Left"), Some(KeyCode::ArrowLeft));
        assert_eq!(key_from_name("ArrowLeft"), Some(KeyCode::ArrowLeft));
        assert_eq!(key_from_name("space"), Some(KeyCode::Space));
        assert_eq!(key_from_name("F5"), Some(KeyCode::F5));
        assert_eq!(key_from_name("F12"), Some(KeyCode::F12));
        assert_eq!(key_from_name("Shift"), Some(KeyCode::ShiftLeft));
    }

    #[test]
    fn unknown_names_are_rejected() {
        for name in ["", "F0", "F13", "Banana", "WW"] {
            assert_eq!(key_from_name(name), None, "{name}");
        }
    }
}
