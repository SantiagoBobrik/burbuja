//! macOS virtual keycodes (`kVK_*`, HIToolbox `Events.h`) ↔ the matcher's [`Key`]s.
//! Keycodes are positional (ANSI layout), like Windows' virtual keys for letters.

use super::vk::OTHER_KEY;
use crate::core::hotkey_spec::{Key, ModKey};

pub const KC_ESCAPE: u16 = 0x35;

pub const MODIFIERS: [(u16, ModKey); 9] = [
    (0x3B, ModKey::LeftCtrl),
    (0x3E, ModKey::RightCtrl),
    (0x38, ModKey::LeftShift),
    (0x3C, ModKey::RightShift),
    (0x3A, ModKey::LeftAlt),
    (0x3D, ModKey::RightAlt),
    (0x37, ModKey::LeftSuper),
    (0x36, ModKey::RightSuper),
    (0x3F, ModKey::Fn),
];

const MAIN: [(u16, &str); 83] = [
    (0x00, "A"),
    (0x0B, "B"),
    (0x08, "C"),
    (0x02, "D"),
    (0x0E, "E"),
    (0x03, "F"),
    (0x05, "G"),
    (0x04, "H"),
    (0x22, "I"),
    (0x26, "J"),
    (0x28, "K"),
    (0x25, "L"),
    (0x2E, "M"),
    (0x2D, "N"),
    (0x1F, "O"),
    (0x23, "P"),
    (0x0C, "Q"),
    (0x0F, "R"),
    (0x01, "S"),
    (0x11, "T"),
    (0x20, "U"),
    (0x09, "V"),
    (0x0D, "W"),
    (0x07, "X"),
    (0x10, "Y"),
    (0x06, "Z"),
    (0x1D, "0"),
    (0x12, "1"),
    (0x13, "2"),
    (0x14, "3"),
    (0x15, "4"),
    (0x17, "5"),
    (0x16, "6"),
    (0x1A, "7"),
    (0x1C, "8"),
    (0x19, "9"),
    (0x7A, "F1"),
    (0x78, "F2"),
    (0x63, "F3"),
    (0x76, "F4"),
    (0x60, "F5"),
    (0x61, "F6"),
    (0x62, "F7"),
    (0x64, "F8"),
    (0x65, "F9"),
    (0x6D, "F10"),
    (0x67, "F11"),
    (0x6F, "F12"),
    (0x69, "F13"),
    (0x6B, "F14"),
    (0x71, "F15"),
    (0x6A, "F16"),
    (0x40, "F17"),
    (0x4F, "F18"),
    (0x50, "F19"),
    (0x5A, "F20"),
    (0x31, "Space"),
    (0x30, "Tab"),
    (0x24, "Enter"),
    (0x33, "Backspace"),
    (0x39, "CapsLock"),
    // The Help key sits where Insert is on PC keyboards.
    (0x72, "Insert"),
    (0x75, "Delete"),
    (0x73, "Home"),
    (0x77, "End"),
    (0x74, "PageUp"),
    (0x79, "PageDown"),
    (0x7E, "Up"),
    (0x7D, "Down"),
    (0x7B, "Left"),
    (0x7C, "Right"),
    (0x32, "Grave"),
    (0x1B, "Minus"),
    (0x18, "Equals"),
    (0x21, "LeftBracket"),
    (0x1E, "RightBracket"),
    (0x2A, "Backslash"),
    (0x29, "Semicolon"),
    (0x27, "Quote"),
    (0x2B, "Comma"),
    (0x2F, "Period"),
    (0x2C, "Slash"),
    // Keypad Enter.
    (0x4C, "Enter"),
];

/// Keys a Mac keyboard doesn't have.
#[cfg(test)]
const NOT_ON_MAC: [&str; 8] = [
    "F21",
    "F22",
    "F23",
    "F24",
    "Pause",
    "ScrollLock",
    "PrintScreen",
    "Menu",
];

/// The key a keyboard event is about. Unknown codes become `Main(OTHER_KEY)`.
pub fn key_from_keycode(code: u16) -> Key {
    if code == KC_ESCAPE {
        return Key::Escape;
    }
    if let Some(&(_, m)) = MODIFIERS.iter().find(|(c, _)| *c == code) {
        return Key::Mod(m);
    }
    Key::Main(
        MAIN.iter()
            .find(|(c, _)| *c == code)
            .map_or(OTHER_KEY, |&(_, id)| id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hotkey_spec::main_keys;

    #[test]
    fn every_mac_key_is_reachable() {
        for id in main_keys().filter(|id| !NOT_ON_MAC.contains(id)) {
            assert!(
                MAIN.iter().any(|&(_, k)| k == id),
                "{id} has no macOS keycode"
            );
        }
        for &(code, id) in &MAIN {
            assert_eq!(key_from_keycode(code), Key::Main(id));
        }
    }

    #[test]
    fn known_codes() {
        assert_eq!(key_from_keycode(0x35), Key::Escape);
        assert_eq!(key_from_keycode(0x3F), Key::Mod(ModKey::Fn));
        assert_eq!(key_from_keycode(0x36), Key::Mod(ModKey::RightSuper));
        assert_eq!(key_from_keycode(0x31), Key::Main("Space"));
        assert_eq!(key_from_keycode(0x7F), Key::Main(OTHER_KEY));
    }
}
