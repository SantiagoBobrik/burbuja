//! Inserting the final text where the user's cursor is.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::time::Duration;

use crate::settings::InsertMethod;

const MODIFIER_WAIT: Duration = Duration::from_millis(1500);
const RESTORE_AFTER: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    Pasted,
    Typed,
    /// The target window is elevated (admin): the text is on the clipboard for Ctrl+V.
    ElevatedTarget,
    /// macOS would drop our keystrokes (no Accessibility, or Secure Input on): the text is on the
    /// clipboard for Cmd+V.
    InputBlocked,
    /// Simulated input failed; the text is on the clipboard.
    ClipboardOnly,
}

/// Blocking (sleeps while waiting for modifiers and before restoring): run it off the
/// controller thread.
#[cfg(windows)]
pub fn insert(text: &str, method: InsertMethod, restore_clipboard: bool) -> Inserted {
    use windows as w;

    if w::foreground_blocks_input() {
        w::write_text(text, false);
        return Inserted::ElevatedTarget;
    }
    if !w::wait_for_modifiers_released(MODIFIER_WAIT) {
        log::warn!("modifiers still held after {MODIFIER_WAIT:?}; inserting anyway");
    }
    match method {
        InsertMethod::Type => {
            // The transcript also stays on the clipboard to paste again.
            w::write_text(text, false);
            if w::type_text(text) {
                Inserted::Typed
            } else {
                Inserted::ClipboardOnly
            }
        }
        InsertMethod::Paste => {
            let snapshot = restore_clipboard.then(w::snapshot).flatten();
            let Some(sequence) = w::write_text(text, restore_clipboard) else {
                return Inserted::ClipboardOnly;
            };
            if !w::send_paste() {
                return Inserted::ClipboardOnly;
            }
            if let Some(snapshot) = snapshot {
                std::thread::sleep(RESTORE_AFTER);
                if !w::restore_if_unchanged(snapshot, sequence) {
                    log::info!("clipboard changed after paste; not restoring");
                }
            }
            Inserted::Pasted
        }
    }
}

/// The app the text is going into, by executable name (history "app" column).
#[cfg(windows)]
pub fn target_app() -> Option<String> {
    windows::foreground_app_name()
}

/// Copies `text` to the clipboard (history "Copy").
#[cfg(windows)]
pub fn copy_to_clipboard(text: &str) -> bool {
    windows::copy(text)
}

/// Blocking, like the Windows version: run it off the controller thread.
#[cfg(target_os = "macos")]
pub fn insert(text: &str, method: InsertMethod, restore_clipboard: bool) -> Inserted {
    use macos as m;

    let _pool = m::Pool::new();
    if m::input_blocked() {
        m::write_text(text, false);
        return Inserted::InputBlocked;
    }
    if !m::wait_for_modifiers_released(MODIFIER_WAIT) {
        log::warn!("modifiers still held after {MODIFIER_WAIT:?}; inserting anyway");
    }
    match method {
        InsertMethod::Type => {
            // The transcript also stays on the clipboard to paste again.
            m::write_text(text, false);
            if m::type_text(text) {
                Inserted::Typed
            } else {
                Inserted::ClipboardOnly
            }
        }
        InsertMethod::Paste => {
            let snapshot = restore_clipboard.then(m::snapshot);
            let Some(change_count) = m::write_text(text, restore_clipboard) else {
                return Inserted::ClipboardOnly;
            };
            if !m::send_paste() {
                return Inserted::ClipboardOnly;
            }
            if let Some(snapshot) = snapshot {
                std::thread::sleep(RESTORE_AFTER);
                if !m::restore_if_unchanged(snapshot, change_count) {
                    log::info!("clipboard changed after paste; not restoring");
                }
            }
            Inserted::Pasted
        }
    }
}

#[cfg(target_os = "macos")]
pub fn target_app() -> Option<String> {
    let _pool = macos::Pool::new();
    macos::foreground_app_name()
}

#[cfg(target_os = "macos")]
pub fn copy_to_clipboard(text: &str) -> bool {
    let _pool = macos::Pool::new();
    macos::write_text(text, false).is_some()
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn target_app() -> Option<String> {
    None
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn copy_to_clipboard(_text: &str) -> bool {
    false
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn insert(_text: &str, _method: InsertMethod, _restore_clipboard: bool) -> Inserted {
    Inserted::ClipboardOnly
}
