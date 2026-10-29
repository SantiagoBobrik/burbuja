//! macOS keyboard source: a Quartz event tap (`CGEventTapCreate`) on a dedicated thread with its
//! own run loop. An active tap needs the Accessibility permission (some setups also Input
//! Monitoring). The thread keeps retrying until macOS allows it, so granting the permission turns
//! the hotkey on without a restart.
//!
//! The FFI below is the documented CoreGraphics / CoreFoundation C API (`CGEventTypes.h`,
//! `CGEvent.h`, `CFRunLoop.h`, `CFMachPort.h`).

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use super::KeyHandler;
use super::mac_keys::key_from_keycode;
use crate::core::hotkey_spec::{Key, ModKey};

type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFStringRef = *const c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

const SESSION_EVENT_TAP: u32 = 1; // kCGSessionEventTap
const HEAD_INSERT_EVENT_TAP: u32 = 0; // kCGHeadInsertEventTap
const TAP_OPTION_DEFAULT: u32 = 0; // kCGEventTapOptionDefault (can swallow)
const KEY_DOWN: u32 = 10; // kCGEventKeyDown
const KEY_UP: u32 = 11; // kCGEventKeyUp
const FLAGS_CHANGED: u32 = 12; // kCGEventFlagsChanged
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const KEYBOARD_EVENT_KEYCODE: u32 = 9; // kCGKeyboardEventKeycode
const EVENT_SOURCE_USER_DATA: u32 = 42; // kCGEventSourceUserData
const TAP_RETRY: Duration = Duration::from_secs(1);

/// Device-dependent modifier flag bits (IOKit `NX_DEVICE*KEYMASK`): one bit per physical side.
const fn side_mask(m: ModKey) -> u64 {
    match m {
        ModKey::LeftCtrl => 0x0000_0001,
        ModKey::LeftShift => 0x0000_0002,
        ModKey::RightShift => 0x0000_0004,
        ModKey::LeftSuper => 0x0000_0008,
        ModKey::RightSuper => 0x0000_0010,
        ModKey::LeftAlt => 0x0000_0020,
        ModKey::RightAlt => 0x0000_0040,
        ModKey::RightCtrl => 0x0000_2000,
        ModKey::Fn => 0x0080_0000, // kCGEventFlagMaskSecondaryFn
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
}

static HANDLER: OnceLock<Arc<KeyHandler>> = OnceLock::new();
/// The tap, so the callback can re-enable it when macOS disables it.
static TAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn install(handler: Arc<KeyHandler>) -> Result<(), String> {
    HANDLER
        .set(handler)
        .map_err(|_| "the keyboard tap is already installed".to_string())?;
    thread::Builder::new()
        .name("burbuja-hotkeys".into())
        .spawn(move || {
            let tap = wait_for_tap();
            TAP.store(tap, Ordering::SeqCst);
            log::info!("keyboard tap active");
            // SAFETY: documented CoreFoundation / CoreGraphics calls; the tap and its run loop
            // source live for the rest of the process.
            unsafe {
                let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                CFRunLoopRun();
            }
            log::warn!("keyboard tap thread ended");
        })
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Blocks until macOS lets the tap be created. It only tries once a permission is on, so that
/// launching doesn't raise a system prompt before the Hub has explained it.
fn wait_for_tap() -> CFMachPortRef {
    let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
    let mut logged = false;
    loop {
        // SAFETY: argument-less status queries, and a documented call with a valid callback.
        unsafe {
            if AXIsProcessTrusted() || CGPreflightListenEventAccess() {
                let tap = CGEventTapCreate(
                    SESSION_EVENT_TAP,
                    HEAD_INSERT_EVENT_TAP,
                    TAP_OPTION_DEFAULT,
                    mask,
                    tap_callback,
                    ptr::null_mut(),
                );
                if !tap.is_null() {
                    return tap;
                }
            }
        }
        if !logged {
            log::warn!("keyboard tap waiting for the macOS permission");
            logged = true;
        }
        thread::sleep(TAP_RETRY);
    }
}

pub fn tap_active() -> bool {
    !TAP.load(Ordering::SeqCst).is_null()
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    if event_type == TAP_DISABLED_BY_TIMEOUT || event_type == TAP_DISABLED_BY_USER_INPUT {
        let tap = TAP.load(Ordering::SeqCst);
        if !tap.is_null() {
            // SAFETY: the tap created in `install`, alive for the process.
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    let Some(handler) = HANDLER.get() else {
        return event;
    };
    // SAFETY: `event` is the live event macOS passes to the callback.
    let (code, flags, source_data) = unsafe {
        (
            CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) as u16,
            CGEventGetFlags(event),
            CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA),
        )
    };
    // Our own paste or typing.
    if source_data == super::INJECTED_MARKER as i64 {
        return event;
    }
    let key = key_from_keycode(code);
    let down = match (event_type, key) {
        (KEY_DOWN, _) => true,
        (KEY_UP, _) => false,
        // A modifier changed: its side bit says whether it is now held.
        (FLAGS_CHANGED, Key::Mod(m)) => flags & side_mask(m) != 0,
        _ => return event,
    };
    if handler.on_key(key, down, Instant::now()) {
        ptr::null_mut() // swallowed
    } else {
        event
    }
}

/// Missed key-ups are rare on macOS (the tap sees the session's events), so nothing is pruned.
pub(super) fn is_key_down(_: Key) -> bool {
    true
}

/// macOS has no Start menu / menu-bar focus on a lone modifier tap.
pub(super) fn send_menu_mask() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkeys::mac_keys::MODIFIERS;

    #[test]
    fn every_modifier_has_its_own_side_bit() {
        let masks: Vec<u64> = MODIFIERS.iter().map(|&(_, m)| side_mask(m)).collect();
        for (i, a) in masks.iter().enumerate() {
            assert_eq!(a.count_ones(), 1);
            assert!(masks[i + 1..].iter().all(|b| a != b));
        }
    }
}
