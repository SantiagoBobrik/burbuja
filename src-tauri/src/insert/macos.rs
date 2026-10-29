//! macOS insertion: pasteboard snapshot/restore, Cmd+V / Unicode typing with CGEvent, modifier
//! wait and Secure Input detection.
//!
//! The FFI below is the documented C / Objective-C API (`CGEvent.h`, `CGEventSource.h`,
//! `AXUIElement.h`, `CarbonEvents.h`, `NSPasteboard.h`, `NSWorkspace.h`, `objc/message.h`).

use std::ffi::{CStr, c_char, c_void};
use std::thread::sleep;
use std::time::{Duration, Instant};

type Id = *mut c_void;
type Sel = *const c_void;
type CGEventRef = *mut c_void;

const HID_SYSTEM_STATE: i32 = 1; // kCGEventSourceStateHIDSystemState
const HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap
const EVENT_SOURCE_USER_DATA: u32 = 42; // kCGEventSourceUserData
const FLAG_COMMAND: u64 = 0x0010_0000; // kCGEventFlagMaskCommand
/// Shift, Control, Option, Command, Fn (`kCGEventFlagMask*`).
const HELD_MODIFIERS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | FLAG_COMMAND | 0x0080_0000;
const KEY_V: u16 = 9; // kVK_ANSI_V
const KEY_RETURN: u16 = 36; // kVK_Return
const UTF8_ENCODING: usize = 4; // NSUTF8StringEncoding
const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventKeyboardSetUnicodeString(event: CGEventRef, length: usize, text: *const u16);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSPasteboardTypeString: Id;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

/// Sends a message with no arguments. `R` must match the method's return type.
unsafe fn send<R>(target: Id, selector: &CStr) -> R {
    // SAFETY: objc_msgSend must be cast to the method's signature; the caller gives `R`.
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel) -> R =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(target, sel_registerName(selector.as_ptr()))
    }
}

/// Sends a message with one object argument.
unsafe fn send1<R>(target: Id, selector: &CStr, a: Id) -> R {
    // SAFETY: as in `send`.
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, Id) -> R =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(target, sel_registerName(selector.as_ptr()), a)
    }
}

/// Sends a message with two object arguments.
unsafe fn send2<R>(target: Id, selector: &CStr, a: Id, b: Id) -> R {
    // SAFETY: as in `send`.
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, Id, Id) -> R =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(target, sel_registerName(selector.as_ptr()), a, b)
    }
}

fn class(name: &CStr) -> Id {
    // SAFETY: a NUL-terminated class name; a null result turns later messages into no-ops.
    unsafe { objc_getClass(name.as_ptr()) }
}

fn ns_string(text: &str) -> Id {
    // SAFETY: `initWithBytes:length:encoding:` copies the bytes; the pool owns the result.
    unsafe {
        let string: Id = send(class(c"NSString"), c"alloc");
        let init: unsafe extern "C" fn(Id, Sel, *const u8, usize, usize) -> Id =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let string = init(
            string,
            sel_registerName(c"initWithBytes:length:encoding:".as_ptr()),
            text.as_ptr(),
            text.len(),
            UTF8_ENCODING,
        );
        send(string, c"autorelease")
    }
}

fn pasteboard() -> Id {
    // SAFETY: `+generalPasteboard` takes no arguments and returns the shared pasteboard.
    unsafe { send(class(c"NSPasteboard"), c"generalPasteboard") }
}

/// Worker threads have no autorelease pool, and the objects Cocoa returns need one. Everything
/// read from the pasteboard stays valid until this is dropped.
pub struct Pool(*mut c_void);

impl Pool {
    pub fn new() -> Self {
        // SAFETY: paired with the pop in `drop`, on the same thread.
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: the token from `new`.
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

/// One `(type, data)` list per pasteboard item. Valid while the caller's [`Pool`] lives.
pub struct Snapshot(Vec<Vec<(Id, Id)>>);

pub fn snapshot() -> Snapshot {
    let mut items = Vec::new();
    // SAFETY: documented NSPasteboard / NSPasteboardItem / NSArray getters.
    unsafe {
        let list: Id = send(pasteboard(), c"pasteboardItems");
        let item_count: usize = send(list, c"count");
        for i in 0..item_count {
            let item: Id = at(list, i);
            let types: Id = send(item, c"types");
            let type_count: usize = send(types, c"count");
            let mut formats = Vec::new();
            for t in 0..type_count {
                let format = at(types, t);
                let data: Id = send1(item, c"dataForType:", format);
                if !data.is_null() {
                    formats.push((format, data));
                }
            }
            items.push(formats);
        }
    }
    Snapshot(items)
}

unsafe fn at(array: Id, index: usize) -> Id {
    // SAFETY: `index` is below the array's count.
    unsafe {
        let f: unsafe extern "C" fn(Id, Sel, usize) -> Id =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(array, sel_registerName(c"objectAtIndex:".as_ptr()), index)
    }
}

/// Puts `text` on the pasteboard. `transient` asks clipboard managers to skip it
/// (nspasteboard.org). Returns the change count after the write.
pub fn write_text(text: &str, transient: bool) -> Option<isize> {
    let board = pasteboard();
    // SAFETY: documented NSPasteboard calls with live NSString arguments.
    unsafe {
        let _: isize = send(board, c"clearContents");
        let written: bool = send2(
            board,
            c"setString:forType:",
            ns_string(text),
            NSPasteboardTypeString,
        );
        if !written {
            return None;
        }
        if transient {
            let _: bool = send2(
                board,
                c"setString:forType:",
                ns_string(""),
                ns_string(TRANSIENT_TYPE),
            );
        }
        Some(send(board, c"changeCount"))
    }
}

/// Restores `snapshot` only if the pasteboard still holds what we wrote.
pub fn restore_if_unchanged(snapshot: Snapshot, change_count: isize) -> bool {
    let board = pasteboard();
    // SAFETY: documented NSPasteboard / NSPasteboardItem / NSMutableArray calls; the snapshot's
    // objects are alive in the caller's pool.
    unsafe {
        let current: isize = send(board, c"changeCount");
        if current != change_count {
            return false;
        }
        let items: Id = send(class(c"NSMutableArray"), c"array");
        for formats in &snapshot.0 {
            let item: Id = send(class(c"NSPasteboardItem"), c"new");
            let item: Id = send(item, c"autorelease");
            for &(format, data) in formats {
                let _: bool = send2(item, c"setData:forType:", data, format);
            }
            let _: () = send1(items, c"addObject:", item);
        }
        let _: isize = send(board, c"clearContents");
        let _: bool = send1(board, c"writeObjects:", items);
    }
    true
}

/// Waits (up to `max`) until no modifier is physically held, so Cmd+V doesn't become Cmd+Opt+V.
/// Returns false on timeout.
pub fn wait_for_modifiers_released(max: Duration) -> bool {
    let started = Instant::now();
    loop {
        // SAFETY: plain state query.
        if unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) } & HELD_MODIFIERS == 0 {
            return true;
        }
        if started.elapsed() >= max {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}

/// True when macOS would silently drop our keystrokes: no Accessibility permission, or a password
/// field has Secure Input on.
pub fn input_blocked() -> bool {
    // SAFETY: argument-less status queries.
    unsafe { !AXIsProcessTrusted() || IsSecureEventInputEnabled() }
}

/// Posts one key transition, marked so our own event tap ignores it.
fn post(key: u16, down: bool, flags: u64, text: Option<&[u16]>) -> bool {
    // SAFETY: a null source is documented as valid; the event is released after posting.
    unsafe {
        let event = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
        if event.is_null() {
            return false;
        }
        CGEventSetFlags(event, flags);
        CGEventSetIntegerValueField(
            event,
            EVENT_SOURCE_USER_DATA,
            crate::hotkeys::INJECTED_MARKER as i64,
        );
        if let Some(units) = text {
            CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
        }
        CGEventPost(HID_EVENT_TAP, event);
        CFRelease(event);
    }
    true
}

pub fn send_paste() -> bool {
    post(KEY_V, true, FLAG_COMMAND, None) && post(KEY_V, false, FLAG_COMMAND, None)
}

/// Types `text` as Unicode key events; `\n` becomes Return.
pub fn type_text(text: &str) -> bool {
    for (i, ch) in text.chars().enumerate() {
        let ok = match ch {
            '\r' => true,
            '\n' => post(KEY_RETURN, true, 0, None) && post(KEY_RETURN, false, 0, None),
            _ => {
                let mut buf = [0u16; 2];
                let units = &*ch.encode_utf16(&mut buf);
                post(0, true, 0, Some(units)) && post(0, false, 0, Some(units))
            }
        };
        if !ok {
            return false;
        }
        if i % 32 == 31 {
            sleep(Duration::from_millis(2));
        }
    }
    true
}

/// The frontmost app's name in lowercase (e.g. `notes`), for history.
pub fn foreground_app_name() -> Option<String> {
    // SAFETY: documented NSWorkspace / NSRunningApplication getters; `UTF8String` stays valid
    // until the pool drains.
    unsafe {
        let workspace: Id = send(class(c"NSWorkspace"), c"sharedWorkspace");
        let app: Id = send(workspace, c"frontmostApplication");
        if app.is_null() {
            return None;
        }
        let name: Id = send(app, c"localizedName");
        if name.is_null() {
            return None;
        }
        let utf8: *const c_char = send(name, c"UTF8String");
        (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().to_lowercase())
    }
}
