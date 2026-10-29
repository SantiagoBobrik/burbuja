//! Windows virtual-key codes ↔ the matcher's [`Key`]s. The low-level keyboard hook reports
//! side-specific modifier codes (`VK_LSHIFT`, `VK_RMENU`, …), so sides map one to one. OEM
//! punctuation keys are named after their US-layout position.

use crate::core::hotkey_spec::{Key, ModKey};

/// Stand-in id for keys outside the table: they only matter as "some other key" (chords).
pub const OTHER_KEY: &str = "Other";

const MODIFIERS: [(u16, ModKey); 8] = [
    (0xA2, ModKey::LeftCtrl),
    (0xA3, ModKey::RightCtrl),
    (0xA0, ModKey::LeftShift),
    (0xA1, ModKey::RightShift),
    (0xA4, ModKey::LeftAlt),
    (0xA5, ModKey::RightAlt),
    (0x5B, ModKey::LeftSuper),
    (0x5C, ModKey::RightSuper),
];

pub const VK_ESCAPE: u16 = 0x1B;

const NAMED: [(u16, &str); 30] = [
    (0x20, "Space"),
    (0x09, "Tab"),
    (0x0D, "Enter"),
    (0x08, "Backspace"),
    (0x14, "CapsLock"),
    (0x13, "Pause"),
    (0x91, "ScrollLock"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    (0x24, "Home"),
    (0x23, "End"),
    (0x21, "PageUp"),
    (0x22, "PageDown"),
    (0x26, "Up"),
    (0x28, "Down"),
    (0x25, "Left"),
    (0x27, "Right"),
    (0x5D, "Menu"),
    (0x2C, "PrintScreen"),
    (0xC0, "Grave"),
    (0xBD, "Minus"),
    (0xBB, "Equals"),
    (0xDB, "LeftBracket"),
    (0xDD, "RightBracket"),
    (0xDC, "Backslash"),
    (0xBA, "Semicolon"),
    (0xDE, "Quote"),
    (0xBC, "Comma"),
    (0xBE, "Period"),
    (0xBF, "Slash"),
];

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const FUNCTION: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

/// The key a hook event is about. Unknown codes become `Main(OTHER_KEY)`.
pub fn key_from_vk(vk: u16) -> Key {
    if vk == VK_ESCAPE {
        return Key::Escape;
    }
    if let Some(&(_, m)) = MODIFIERS.iter().find(|(code, _)| *code == vk) {
        return Key::Mod(m);
    }
    let id = match vk {
        0x41..=0x5A => LETTERS[usize::from(vk - 0x41)],
        0x30..=0x39 => DIGITS[usize::from(vk - 0x30)],
        0x70..=0x87 => FUNCTION[usize::from(vk - 0x70)],
        _ => NAMED
            .iter()
            .find(|(code, _)| *code == vk)
            .map_or(OTHER_KEY, |&(_, id)| id),
    };
    Key::Main(id)
}

/// The virtual-key code of a key, for `GetAsyncKeyState`. `None` for `Fn` and unknown keys.
pub fn vk_from_key(key: Key) -> Option<u16> {
    match key {
        Key::Escape => Some(VK_ESCAPE),
        Key::Mod(m) => MODIFIERS
            .iter()
            .find(|(_, k)| *k == m)
            .map(|&(code, _)| code),
        Key::Main(id) => {
            let index = |list: &[&str]| list.iter().position(|&k| k == id).map(|i| i as u16);
            index(&LETTERS)
                .map(|i| 0x41 + i)
                .or_else(|| index(&DIGITS).map(|i| 0x30 + i))
                .or_else(|| index(&FUNCTION).map(|i| 0x70 + i))
                .or_else(|| NAMED.iter().find(|(_, k)| *k == id).map(|&(code, _)| code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hotkey_spec::main_keys;

    #[test]
    fn every_main_key_has_a_code_and_maps_back() {
        for id in main_keys() {
            let vk = vk_from_key(Key::Main(id)).unwrap_or_else(|| panic!("{id} has no VK"));
            assert_eq!(key_from_vk(vk), Key::Main(id), "{id}");
        }
    }

    #[test]
    fn modifiers_keep_their_side() {
        assert_eq!(key_from_vk(0xA5), Key::Mod(ModKey::RightAlt));
        assert_eq!(key_from_vk(0xA2), Key::Mod(ModKey::LeftCtrl));
        assert_eq!(key_from_vk(0x5B), Key::Mod(ModKey::LeftSuper));
        for (code, m) in MODIFIERS {
            assert_eq!(vk_from_key(Key::Mod(m)), Some(code));
        }
        assert_eq!(vk_from_key(Key::Mod(ModKey::Fn)), None);
    }

    #[test]
    fn known_codes() {
        assert_eq!(key_from_vk(0x1B), Key::Escape);
        assert_eq!(key_from_vk(0x20), Key::Main("Space"));
        assert_eq!(key_from_vk(0x44), Key::Main("D"));
        assert_eq!(key_from_vk(0x78), Key::Main("F9"));
        assert_eq!(key_from_vk(0x2E), Key::Main("Delete"));
        assert_eq!(key_from_vk(0x08), Key::Main("Backspace"));
        // Media keys and the like are just "another key".
        assert_eq!(key_from_vk(0xB3), Key::Main(OTHER_KEY));
    }
}
