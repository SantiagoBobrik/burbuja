//! Windows keyboard source: a `WH_KEYBOARD_LL` hook on a dedicated thread with its own message
//! loop (the hook procedure runs inside that loop).

use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::Instant;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_UP, MSG, SetWindowsHookExW,
    WH_KEYBOARD_LL,
};

use super::vk::{key_from_vk, vk_from_key};
use super::{INJECTED_MARKER, KeyHandler};
use crate::core::hotkey_spec::Key;

/// An unassigned virtual key: pressing it between Win/Alt down and up keeps Windows from
/// reading the release as a lone tap (which opens Start or focuses the menu bar).
const MENU_MASK_VK: u16 = 0xE8;

/// The hook procedure has no user data argument, so the handler lives here.
static HANDLER: OnceLock<Arc<KeyHandler>> = OnceLock::new();

pub(super) fn install(handler: Arc<KeyHandler>) -> Result<(), String> {
    HANDLER
        .set(handler)
        .map_err(|_| "the keyboard hook is already installed".to_string())?;
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("burbuja-hotkeys".into())
        .spawn(move || {
            // SAFETY: our own module handle and a hook procedure with the required signature.
            let hooked = unsafe {
                GetModuleHandleW(None).and_then(|module| {
                    SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(hook_proc),
                        Some(HINSTANCE(module.0)),
                        0,
                    )
                })
            };
            let ok = hooked.is_ok();
            let _ = ready_tx.send(hooked.map(drop).map_err(|e| e.to_string()));
            if !ok {
                return;
            }
            let mut msg = MSG::default();
            // SAFETY: a plain message loop on this thread; nothing posts to it, it just keeps
            // the thread alive and lets Windows call the hook.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {}
            log::warn!("keyboard hook thread ended");
        })
        .map_err(|e| e.to_string())?;
    ready_rx
        .recv()
        .map_err(|_| "keyboard hook thread ended".to_string())?
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(handler) = HANDLER.get()
    {
        // SAFETY: for HC_ACTION, lparam points to the event's KBDLLHOOKSTRUCT.
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if event.dwExtraInfo != INJECTED_MARKER {
            let down = !event.flags.contains(LLKHF_UP);
            let key = key_from_vk(event.vkCode as u16);
            if handler.on_key(key, down, Instant::now()) {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Whether `key` is physically held, for dropping keys whose key-up was missed (UAC prompt,
/// lock screen). Keys without a virtual-key code count as held.
pub(super) fn is_key_down(key: Key) -> bool {
    vk_from_key(key).is_none_or(|vk| {
        // SAFETY: plain key state query.
        let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
        state as u16 & 0x8000 != 0
    })
}

/// Injects a press and release of [`MENU_MASK_VK`], marked so our hook ignores it.
pub(super) fn send_menu_mask() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MENU_MASK_VK),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_MARKER,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: SendInput with a stack array of fully initialized INPUT structs.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        log::warn!("menu mask injection failed");
    }
}
