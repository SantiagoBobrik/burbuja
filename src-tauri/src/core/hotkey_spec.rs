//! Hotkey parsing and matching.
//!
//! A [`HotkeySpec`] is parsed from either Burbuja's canonical syntax (`"Ctrl+Super"`,
//! `"RightAlt"`, `"Ctrl+Shift+Space"`, `"Fn"`) or the lowercase aliases (`"ctrl+win"`, `"right_alt"`).
//! [`HotkeyMatcher`] turns raw key transitions into hotkey events: chord detection, Esc handling
//! and stale-key pruning live here. The `hotkeys/` module feeds it from the platform keyboard
//! hook and swallows the keys it says to.

use std::collections::HashSet;
use std::fmt;

/// A physical modifier key. Side-specific, like the Windows low-level hook reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModKey {
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    LeftSuper,
    RightSuper,
    /// macOS only: on Windows Fn is handled by keyboard firmware and never reaches the OS.
    Fn,
}

/// A key as the matcher sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Mod(ModKey),
    Escape,
    /// A non-modifier key, by its canonical id (`"Space"`, `"F9"`, `"A"`).
    Main(&'static str),
}

impl Key {
    pub fn is_modifier(self) -> bool {
        matches!(self, Key::Mod(_))
    }
}

/// One `+`-separated modifier in a hotkey: either side (`Ctrl`) or a specific side (`RightAlt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModGroup {
    Ctrl,
    Shift,
    Alt,
    Super,
    Fn,
    Side(ModKey),
}

impl ModGroup {
    pub fn keys(self) -> &'static [ModKey] {
        use ModKey::*;
        match self {
            ModGroup::Ctrl => &[LeftCtrl, RightCtrl],
            ModGroup::Shift => &[LeftShift, RightShift],
            ModGroup::Alt => &[LeftAlt, RightAlt],
            ModGroup::Super => &[LeftSuper, RightSuper],
            ModGroup::Fn => &[Fn],
            ModGroup::Side(k) => match k {
                LeftCtrl => &[LeftCtrl],
                RightCtrl => &[RightCtrl],
                LeftShift => &[LeftShift],
                RightShift => &[RightShift],
                LeftAlt => &[LeftAlt],
                RightAlt => &[RightAlt],
                LeftSuper => &[LeftSuper],
                RightSuper => &[RightSuper],
                Fn => &[Fn],
            },
        }
    }

    fn canonical(self) -> &'static str {
        match self {
            ModGroup::Ctrl => "Ctrl",
            ModGroup::Shift => "Shift",
            ModGroup::Alt => "Alt",
            ModGroup::Super => "Super",
            ModGroup::Fn | ModGroup::Side(ModKey::Fn) => "Fn",
            ModGroup::Side(k) => side_name(k),
        }
    }

    fn pretty(self, platform: Platform) -> String {
        let base = |k: ModKey| -> &'static str {
            match (k, platform) {
                (ModKey::LeftCtrl | ModKey::RightCtrl, _) => "Ctrl",
                (ModKey::LeftShift | ModKey::RightShift, _) => "Shift",
                (ModKey::LeftAlt | ModKey::RightAlt, Platform::Windows) => "Alt",
                (ModKey::LeftAlt | ModKey::RightAlt, Platform::MacOs) => "Option",
                (ModKey::LeftSuper | ModKey::RightSuper, Platform::Windows) => "Win",
                (ModKey::LeftSuper | ModKey::RightSuper, Platform::MacOs) => "Cmd",
                (ModKey::Fn, _) => "Fn",
            }
        };
        match self {
            ModGroup::Ctrl => base(ModKey::LeftCtrl).into(),
            ModGroup::Shift => base(ModKey::LeftShift).into(),
            ModGroup::Alt => base(ModKey::LeftAlt).into(),
            ModGroup::Super => base(ModKey::LeftSuper).into(),
            ModGroup::Fn | ModGroup::Side(ModKey::Fn) => "Fn".into(),
            ModGroup::Side(k) => {
                let side = if side_name(k).starts_with("Left") {
                    "Left"
                } else {
                    "Right"
                };
                format!("{side} {}", base(k))
            }
        }
    }
}

/// Canonical name of a side-specific modifier.
fn side_name(k: ModKey) -> &'static str {
    match k {
        ModKey::LeftCtrl => "LeftCtrl",
        ModKey::RightCtrl => "RightCtrl",
        ModKey::LeftShift => "LeftShift",
        ModKey::RightShift => "RightShift",
        ModKey::LeftAlt => "LeftAlt",
        ModKey::RightAlt => "RightAlt",
        ModKey::LeftSuper => "LeftSuper",
        ModKey::RightSuper => "RightSuper",
        ModKey::Fn => "Fn",
    }
}

/// How hotkeys are shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Windows
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("Hotkey is empty.")]
    Empty,
    #[error("'{0}' is listed twice.")]
    Duplicate(String),
    #[error("Only one non-modifier key is allowed.")]
    TwoMainKeys,
    #[error("Unknown key '{0}'. Use names like Ctrl, Win, Alt, Shift, RightAlt, Space, F9, A, 1.")]
    Unknown(String),
}

/// A parsed hotkey. Modifier-only hotkeys are watched passively; hotkeys with a main key
/// swallow that key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    groups: Vec<ModGroup>,
    main: Option<&'static str>,
}

impl HotkeySpec {
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let tokens: Vec<&str> = text
            .split('+')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return Err(HotkeyError::Empty);
        }
        let mut groups: Vec<ModGroup> = Vec::new();
        let mut main = None;
        for token in tokens {
            let norm = normalize(token);
            if let Some(group) = parse_modifier(&norm) {
                if groups
                    .iter()
                    .any(|g| g.keys().iter().any(|k| group.keys().contains(k)))
                {
                    return Err(HotkeyError::Duplicate(token.to_string()));
                }
                groups.push(group);
            } else if let Some(def) = MAIN_KEYS.iter().find(|d| d.matches(&norm)) {
                if main.is_some() {
                    return Err(HotkeyError::TwoMainKeys);
                }
                main = Some(def.id);
            } else {
                return Err(HotkeyError::Unknown(token.to_string()));
            }
        }
        Ok(Self { groups, main })
    }

    pub fn groups(&self) -> &[ModGroup] {
        &self.groups
    }

    pub fn main_key(&self) -> Option<&'static str> {
        self.main
    }

    /// Canonical form stored in settings, e.g. `"Ctrl+Super"`, `"RightAlt"`.
    pub fn canonical(&self) -> String {
        self.join(|g| g.canonical().to_string(), |m| m.to_string())
    }

    /// What the user sees, e.g. `"Ctrl+Win"` on Windows, `"Ctrl+Cmd"` on macOS.
    pub fn display(&self, platform: Platform) -> String {
        self.join(|g| g.pretty(platform), |m| m.to_string())
    }

    /// Win or Alt released alone opens the Start menu / menu bar, so they need the mask key.
    pub fn needs_menu_mask(&self) -> bool {
        self.groups.iter().any(|g| {
            g.keys().iter().any(|k| {
                matches!(
                    k,
                    ModKey::LeftAlt | ModKey::RightAlt | ModKey::LeftSuper | ModKey::RightSuper
                )
            })
        })
    }

    pub fn is_hotkey_key(&self, key: Key) -> bool {
        match key {
            Key::Main(m) => self.main == Some(m),
            Key::Mod(k) => self.groups.iter().any(|g| g.keys().contains(&k)),
            Key::Escape => false,
        }
    }

    fn join(&self, group: impl Fn(ModGroup) -> String, main: impl Fn(&str) -> String) -> String {
        let mut parts: Vec<String> = self.groups.iter().map(|g| group(*g)).collect();
        if let Some(m) = self.main {
            parts.push(main(m));
        }
        parts.join("+")
    }
}

impl fmt::Display for HotkeySpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// Lowercase; multi-character tokens also drop `_`, `-` and spaces (`right_alt` == `RightAlt`).
fn normalize(token: &str) -> String {
    let lower = token.to_lowercase();
    if lower.chars().count() > 1 {
        lower
            .chars()
            .filter(|c| !matches!(c, '_' | '-' | ' '))
            .collect()
    } else {
        lower
    }
}

fn parse_modifier(norm: &str) -> Option<ModGroup> {
    use ModKey::*;
    Some(match norm {
        "ctrl" | "control" => ModGroup::Ctrl,
        "shift" => ModGroup::Shift,
        "alt" | "option" | "opt" => ModGroup::Alt,
        "win" | "windows" | "super" | "meta" | "cmd" | "command" => ModGroup::Super,
        "fn" | "function" => ModGroup::Fn,
        "leftctrl" | "lctrl" | "ctrlleft" | "leftcontrol" => ModGroup::Side(LeftCtrl),
        "rightctrl" | "rctrl" | "ctrlright" | "rightcontrol" => ModGroup::Side(RightCtrl),
        "leftshift" | "lshift" | "shiftleft" => ModGroup::Side(LeftShift),
        "rightshift" | "rshift" | "shiftright" => ModGroup::Side(RightShift),
        "leftalt" | "lalt" | "altleft" | "leftoption" => ModGroup::Side(LeftAlt),
        "rightalt" | "ralt" | "altright" | "rightoption" | "altgr" => ModGroup::Side(RightAlt),
        "leftwin" | "lwin" | "winleft" | "leftsuper" | "superleft" | "leftcmd" => {
            ModGroup::Side(LeftSuper)
        }
        "rightwin" | "rwin" | "winright" | "rightsuper" | "superright" | "rightcmd" => {
            ModGroup::Side(RightSuper)
        }
        _ => return None,
    })
}

/// A non-modifier key: canonical id and accepted aliases.
struct MainKeyDef {
    id: &'static str,
    aliases: &'static [&'static str],
}

impl MainKeyDef {
    fn matches(&self, norm: &str) -> bool {
        normalize(self.id) == norm || self.aliases.contains(&norm)
    }
}

macro_rules! plain_keys {
    ($($id:literal),* $(,)?) => { [$(MainKeyDef { id: $id, aliases: &[] }),*] };
}

static LETTERS_DIGITS_F: [MainKeyDef; 60] = plain_keys![
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "F1",
    "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

const fn k(id: &'static str, aliases: &'static [&'static str]) -> MainKeyDef {
    MainKeyDef { id, aliases }
}

static SPECIAL_KEYS: [MainKeyDef; 30] = [
    k("Space", &[]),
    k("Tab", &[]),
    k("Enter", &["return"]),
    k("Backspace", &[]),
    k("CapsLock", &[]),
    k("Pause", &[]),
    k("ScrollLock", &[]),
    k("Insert", &[]),
    k("Delete", &[]),
    k("Home", &[]),
    k("End", &[]),
    k("PageUp", &[]),
    k("PageDown", &[]),
    k("Up", &[]),
    k("Down", &[]),
    k("Left", &[]),
    k("Right", &[]),
    k("Menu", &["apps"]),
    k("PrintScreen", &[]),
    k("Grave", &["`", "backtick"]),
    k("Minus", &["-"]),
    k("Equals", &["="]),
    k("LeftBracket", &["["]),
    k("RightBracket", &["]"]),
    k("Backslash", &["\\"]),
    k("Semicolon", &[";"]),
    k("Quote", &["'"]),
    k("Comma", &[","]),
    k("Period", &["."]),
    k("Slash", &["/"]),
];

struct MainKeys;

impl MainKeys {
    fn iter(&self) -> impl Iterator<Item = &'static MainKeyDef> {
        LETTERS_DIGITS_F.iter().chain(SPECIAL_KEYS.iter())
    }
}

const MAIN_KEYS: MainKeys = MainKeys;

/// Every main key's canonical id, for the platform key tables in `hotkeys/`.
pub fn main_keys() -> impl Iterator<Item = &'static str> {
    MAIN_KEYS.iter().map(|d| d.id)
}

/// Looks up the canonical id of a main key by any of its names (for the `hotkeys/` wrapper).
pub fn main_key_id(name: &str) -> Option<&'static str> {
    let norm = normalize(name);
    MAIN_KEYS.iter().find(|d| d.matches(&norm)).map(|d| d.id)
}

/// "Record new shortcut": collects everything held, finishes when all keys are up.
/// Modifiers are recorded side-agnostic (`Ctrl`), except a lone right Alt (`RightAlt`).
/// Esc alone cancels.
#[derive(Debug, Default)]
pub struct HotkeyRecorder {
    held: HashSet<Key>,
    mods: Vec<ModKey>,
    main: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOutcome {
    Recorded(String),
    Cancelled,
}

impl HotkeyRecorder {
    pub fn feed(&mut self, key: Key, is_down: bool) -> Option<RecordOutcome> {
        if is_down {
            if key == Key::Escape && self.held.is_empty() {
                return Some(RecordOutcome::Cancelled);
            }
            self.held.insert(key);
            match key {
                Key::Mod(m) if !self.mods.contains(&m) => self.mods.push(m),
                Key::Main(id) => self.main = Some(id),
                _ => {}
            }
            return None;
        }
        self.held.remove(&key);
        if !self.held.is_empty() || (self.mods.is_empty() && self.main.is_none()) {
            return None;
        }
        let outcome = self.finish();
        *self = HotkeyRecorder::default();
        Some(outcome)
    }

    fn finish(&self) -> RecordOutcome {
        use ModKey::*;
        let mut groups: Vec<&str> = Vec::new();
        let lone_right_alt = self.main.is_none() && self.mods == [RightAlt];
        for m in &self.mods {
            let g = match m {
                LeftCtrl | RightCtrl => "Ctrl",
                LeftShift | RightShift => "Shift",
                RightAlt if lone_right_alt => "RightAlt",
                LeftAlt | RightAlt => "Alt",
                LeftSuper | RightSuper => "Super",
                Fn => "Fn",
            };
            if !groups.contains(&g) {
                groups.push(g);
            }
        }
        // Canonical order: Ctrl, Alt, Shift, Super, Fn, then the main key.
        let order = ["Ctrl", "Alt", "RightAlt", "Shift", "Super", "Fn"];
        groups.sort_by_key(|g| order.iter().position(|o| o == g));
        let mut parts: Vec<&str> = groups;
        if let Some(main) = self.main.filter(|m| *m != "Other") {
            parts.push(main);
        }
        if parts.is_empty() {
            return RecordOutcome::Cancelled;
        }
        let text = parts.join("+");
        match HotkeySpec::parse(&text) {
            Ok(spec) => RecordOutcome::Recorded(spec.canonical()),
            Err(_) => RecordOutcome::Cancelled,
        }
    }
}

/// Known clashes with system shortcuts, shown as a warning when recording.
pub fn known_conflict(canonical: &str) -> Option<&'static str> {
    match canonical {
        "Super+H" => Some("Windows voice typing"),
        "Super+L" => Some("Lock screen"),
        "Super+D" => Some("Show desktop"),
        "Super+V" => Some("Clipboard history"),
        "Ctrl+Alt+Delete" => Some("Security screen"),
        "RightAlt" | "Alt" => Some("AltGr / menu bar on many layouts"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
    /// Another key was pressed while a modifier-only hotkey was held (e.g. Ctrl+Win+D).
    Chord,
    Escape,
}

impl fmt::Display for HotkeyEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

type PhysicalState = Box<dyn Fn(Key) -> bool + Send>;

/// Turns raw key transitions into hotkey events. Pure logic; runs on the hook thread.
pub struct HotkeyMatcher {
    spec: HotkeySpec,
    down: HashSet<Key>,
    is_physically_down: PhysicalState,
    active: bool,
}

impl HotkeyMatcher {
    pub fn new(spec: HotkeySpec) -> Self {
        Self::with_physical_state(spec, |_| true)
    }

    /// `is_physically_down` lets the matcher drop keys whose key-up was missed (UAC prompt,
    /// lock screen). On Windows it wraps `GetAsyncKeyState`.
    pub fn with_physical_state(
        spec: HotkeySpec,
        is_physically_down: impl Fn(Key) -> bool + Send + 'static,
    ) -> Self {
        Self {
            spec,
            down: HashSet::new(),
            is_physically_down: Box::new(is_physically_down),
            active: false,
        }
    }

    pub fn spec(&self) -> &HotkeySpec {
        &self.spec
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn set_spec(&mut self, spec: HotkeySpec) {
        if spec != self.spec {
            self.spec = spec;
            self.active = false;
        }
    }

    /// Feeds one key transition. Returns true when the key event must be swallowed.
    pub fn process(&mut self, key: Key, is_down: bool, events: &mut Vec<HotkeyEvent>) -> bool {
        if !is_down {
            self.down.remove(&key);
            if !self.active {
                return false;
            }
            if let Some(main) = self.spec.main {
                if key != Key::Main(main) {
                    return false;
                }
                self.active = false;
                events.push(HotkeyEvent::Released);
                return true;
            }
            if self.spec.is_hotkey_key(key) && !self.groups_satisfied() {
                self.active = false;
                events.push(HotkeyEvent::Released);
            }
            return false;
        }

        // Key-ups can be missed (UAC prompt, lock screen), so drop keys that are no longer held.
        let physical = &self.is_physically_down;
        self.down.retain(|&k| k == key || physical(k));
        let repeat = !self.down.insert(key);

        if key == Key::Escape {
            if !repeat {
                events.push(HotkeyEvent::Escape);
            }
            // While the hotkey is held, Esc is part of our gesture (and Ctrl+Esc would open Start).
            return self.active;
        }

        if let Some(main) = self.spec.main {
            if key == Key::Main(main) {
                if self.active {
                    return true; // autorepeat while held
                }
                if repeat || !self.modifiers_exact() {
                    return false;
                }
                self.active = true;
                events.push(HotkeyEvent::Pressed);
                return true;
            }
            if self.active && !repeat && !key.is_modifier() {
                events.push(HotkeyEvent::Chord);
            }
            return false;
        }

        if self.active {
            if !repeat && !self.spec.is_hotkey_key(key) {
                events.push(HotkeyEvent::Chord);
            }
            return false;
        }
        if !repeat
            && self.spec.is_hotkey_key(key)
            && self.groups_satisfied()
            && self.down.iter().all(|&k| self.spec.is_hotkey_key(k))
        {
            self.active = true;
            events.push(HotkeyEvent::Pressed);
        }
        false
    }

    fn groups_satisfied(&self) -> bool {
        self.spec
            .groups
            .iter()
            .all(|g| g.keys().iter().any(|k| self.down.contains(&Key::Mod(*k))))
    }

    fn modifiers_exact(&self) -> bool {
        self.groups_satisfied()
            && self
                .down
                .iter()
                .filter(|k| k.is_modifier())
                .all(|&k| self.spec.is_hotkey_key(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const CTRL: Key = Key::Mod(ModKey::LeftCtrl);
    const WIN: Key = Key::Mod(ModKey::LeftSuper);
    const SHIFT: Key = Key::Mod(ModKey::LeftShift);
    const ESC: Key = Key::Escape;
    const SPACE: Key = Key::Main("Space");
    const D: Key = Key::Main("D");
    const V: Key = Key::Main("V");

    fn parse(s: &str) -> HotkeySpec {
        HotkeySpec::parse(s).unwrap()
    }

    fn feed(m: &mut HotkeyMatcher, keys: &[(Key, bool)]) -> String {
        let mut all = Vec::new();
        for &(key, down) in keys {
            m.process(key, down, &mut all);
        }
        s(&all)
    }

    fn s(events: &[HotkeyEvent]) -> String {
        events
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    // ---- parsing ----

    #[test]
    fn parse_ctrl_win() {
        assert_eq!(parse("ctrl+win").display(Platform::Windows), "Ctrl+Win");
    }

    #[test]
    fn right_alt_is_modifier_only() {
        assert_eq!(parse("Right_Alt").main_key(), None);
    }

    #[test]
    fn combo_main_key() {
        assert_eq!(parse("ctrl+shift+space").main_key(), Some("Space"));
    }

    #[test]
    fn f9() {
        assert_eq!(parse("f9").main_key(), Some("F9"));
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(HotkeySpec::parse(""), Err(HotkeyError::Empty));
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(matches!(
            HotkeySpec::parse("ctrl+bogus"),
            Err(HotkeyError::Unknown(_))
        ));
    }

    #[test]
    fn rejects_two_main_keys() {
        assert_eq!(HotkeySpec::parse("a+b"), Err(HotkeyError::TwoMainKeys));
    }

    #[test]
    fn rejects_duplicate_modifier() {
        assert!(matches!(
            HotkeySpec::parse("ctrl+ctrl"),
            Err(HotkeyError::Duplicate(_))
        ));
    }

    // ---- modifier-only hotkey ----

    #[test]
    fn ctrl_win_press_release() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[(CTRL, true), (WIN, true), (WIN, false), (CTRL, false)]
            ),
            "Pressed,Released"
        );
    }

    #[test]
    fn either_order_autorepeat_ignored() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[
                    (WIN, true),
                    (CTRL, true),
                    (CTRL, true),
                    (CTRL, true),
                    (CTRL, false),
                    (WIN, false)
                ]
            ),
            "Pressed,Released"
        );
    }

    #[test]
    fn ctrl_win_d_is_a_chord() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[
                    (CTRL, true),
                    (WIN, true),
                    (D, true),
                    (D, false),
                    (WIN, false),
                    (CTRL, false)
                ]
            ),
            "Pressed,Chord,Released"
        );
    }

    #[test]
    fn extra_modifier_held_no_trigger() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[
                    (SHIFT, true),
                    (CTRL, true),
                    (WIN, true),
                    (WIN, false),
                    (CTRL, false),
                    (SHIFT, false)
                ]
            ),
            ""
        );
    }

    #[test]
    fn ctrl_held_after_ctrl_v_then_win_triggers() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[
                    (CTRL, true),
                    (V, true),
                    (V, false),
                    (WIN, true),
                    (WIN, false),
                    (CTRL, false)
                ]
            ),
            "Pressed,Released"
        );
    }

    #[test]
    fn win_while_v_still_held_no_trigger() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert_eq!(
            feed(
                &mut m,
                &[
                    (CTRL, true),
                    (V, true),
                    (WIN, true),
                    (WIN, false),
                    (V, false),
                    (CTRL, false)
                ]
            ),
            ""
        );
    }

    #[test]
    fn esc_while_hotkey_held_is_event_and_swallowed() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        let mut events = Vec::new();
        m.process(CTRL, true, &mut events);
        m.process(WIN, true, &mut events);
        let swallowed = m.process(ESC, true, &mut events);
        m.process(ESC, false, &mut events);
        m.process(WIN, false, &mut events);
        m.process(CTRL, false, &mut events);
        assert_eq!(s(&events), "Pressed,Escape,Released");
        assert!(swallowed, "Ctrl+Esc would open Start");
    }

    #[test]
    fn esc_otherwise_passes_through() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        let mut events = Vec::new();
        let swallowed = m.process(ESC, true, &mut events);
        m.process(ESC, false, &mut events);
        assert_eq!(s(&events), "Escape");
        assert!(!swallowed);
    }

    #[test]
    fn modifier_hotkey_never_swallows_modifiers() {
        let mut m = HotkeyMatcher::new(parse("ctrl+win"));
        assert!(!m.process(CTRL, true, &mut Vec::new()));
    }

    #[test]
    fn stale_key_down_pruned() {
        let physically_down: Arc<Mutex<HashSet<Key>>> = Arc::default();
        let probe = Arc::clone(&physically_down);
        let mut m = HotkeyMatcher::with_physical_state(parse("ctrl+win"), move |k| {
            probe.lock().unwrap().contains(&k)
        });
        m.process(SHIFT, true, &mut Vec::new()); // shift-up never arrives
        physically_down.lock().unwrap().extend([CTRL, WIN]);
        assert_eq!(
            feed(&mut m, &[(CTRL, true), (WIN, true), (WIN, false)]),
            "Pressed,Released"
        );
    }

    // ---- combo hotkey with main key ----

    #[test]
    fn combo_main_key_swallowed_modifiers_released_early_ok() {
        let mut m = HotkeyMatcher::new(parse("ctrl+shift+space"));
        let mut ev = Vec::new();
        m.process(CTRL, true, &mut ev);
        m.process(SHIFT, true, &mut ev);
        let swallow_down = m.process(SPACE, true, &mut ev);
        let swallow_repeat = m.process(SPACE, true, &mut ev);
        m.process(SHIFT, false, &mut ev);
        let swallow_up = m.process(SPACE, false, &mut ev);
        m.process(CTRL, false, &mut ev);
        assert_eq!(s(&ev), "Pressed,Released");
        assert!(swallow_down && swallow_repeat && swallow_up);
    }

    #[test]
    fn plain_space_passes_through() {
        let mut m = HotkeyMatcher::new(parse("ctrl+shift+space"));
        assert!(!m.process(SPACE, true, &mut Vec::new()));
    }

    #[test]
    fn ctrl_shift_space_does_not_fire_ctrl_space() {
        let mut m = HotkeyMatcher::new(parse("ctrl+space"));
        assert_eq!(
            feed(
                &mut m,
                &[(CTRL, true), (SHIFT, true), (SPACE, true), (SPACE, false)]
            ),
            ""
        );
    }

    // ---- Burbuja additions ----

    #[test]
    fn lowercase_aliases_convert_to_canonical() {
        let canonical = |s: &str| HotkeySpec::parse(s).unwrap().canonical();
        assert_eq!(canonical("ctrl+win"), "Ctrl+Super");
        assert_eq!(canonical("right_alt"), "RightAlt");
        assert_eq!(canonical("ctrl+shift+space"), "Ctrl+Shift+Space");
        assert_eq!(canonical("f9"), "F9");
    }

    #[test]
    fn canonical_round_trips() {
        for s in [
            "Ctrl+Super",
            "RightAlt",
            "Ctrl+Shift+Space",
            "F9",
            "Fn",
            "LeftCtrl+Grave",
        ] {
            assert_eq!(parse(s).canonical(), s);
        }
    }

    #[test]
    fn display_per_platform() {
        assert_eq!(parse("Ctrl+Super").display(Platform::MacOs), "Ctrl+Cmd");
        assert_eq!(parse("RightAlt").display(Platform::Windows), "Right Alt");
        assert_eq!(parse("RightAlt").display(Platform::MacOs), "Right Option");
        assert_eq!(parse("Fn").display(Platform::MacOs), "Fn");
    }

    #[test]
    fn menu_mask_only_for_win_or_alt() {
        assert!(parse("Ctrl+Super").needs_menu_mask());
        assert!(parse("RightAlt").needs_menu_mask());
        assert!(!parse("Ctrl+Shift+Space").needs_menu_mask());
        assert!(!parse("F9").needs_menu_mask());
    }

    #[test]
    fn side_and_either_side_overlap_is_a_duplicate() {
        assert!(matches!(
            HotkeySpec::parse("ctrl+left_ctrl"),
            Err(HotkeyError::Duplicate(_))
        ));
    }

    #[test]
    fn right_side_hotkey_ignores_left_side() {
        let mut m = HotkeyMatcher::new(parse("RightAlt"));
        let ralt = Key::Mod(ModKey::RightAlt);
        let lalt = Key::Mod(ModKey::LeftAlt);
        assert_eq!(feed(&mut m, &[(lalt, true), (lalt, false)]), "");
        assert_eq!(
            feed(&mut m, &[(ralt, true), (ralt, false)]),
            "Pressed,Released"
        );
    }

    fn record(keys: &[(Key, bool)]) -> Option<RecordOutcome> {
        let mut r = HotkeyRecorder::default();
        keys.iter().find_map(|&(k, down)| r.feed(k, down))
    }

    #[test]
    fn records_a_modifier_only_combo_side_agnostic() {
        let rctrl = Key::Mod(ModKey::RightCtrl);
        assert_eq!(
            record(&[(rctrl, true), (WIN, true), (WIN, false), (rctrl, false)]),
            Some(RecordOutcome::Recorded("Ctrl+Super".into()))
        );
    }

    #[test]
    fn records_a_combo_with_main_key_in_canonical_order() {
        assert_eq!(
            record(&[
                (SHIFT, true),
                (CTRL, true),
                (SPACE, true),
                (SPACE, false),
                (CTRL, false),
                (SHIFT, false)
            ]),
            Some(RecordOutcome::Recorded("Ctrl+Shift+Space".into()))
        );
    }

    #[test]
    fn lone_right_alt_keeps_its_side_and_esc_cancels() {
        let ralt = Key::Mod(ModKey::RightAlt);
        assert_eq!(
            record(&[(ralt, true), (ralt, false)]),
            Some(RecordOutcome::Recorded("RightAlt".into()))
        );
        assert_eq!(record(&[(ESC, true)]), Some(RecordOutcome::Cancelled));
        assert_eq!(
            record(&[(Key::Main("F9"), true), (Key::Main("F9"), false)]),
            Some(RecordOutcome::Recorded("F9".into()))
        );
    }

    #[test]
    fn conflicts_are_flagged() {
        assert!(known_conflict("Super+H").is_some());
        assert!(known_conflict("Ctrl+Super").is_none());
    }

    #[test]
    fn main_key_lookup_by_alias() {
        assert_eq!(main_key_id("return"), Some("Enter"));
        assert_eq!(main_key_id("`"), Some("Grave"));
        assert_eq!(main_key_id("page_up"), Some("PageUp"));
        assert_eq!(main_key_id("escape"), None);
    }
}
