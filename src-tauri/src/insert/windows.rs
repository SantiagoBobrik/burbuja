//! Windows insertion: clipboard (all formats) snapshot/restore, Ctrl+V / Unicode typing with
//! SendInput, modifier wait and UIPI (elevated target) detection.

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL, HWND};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::w;

const CF_UNICODETEXT: u32 = 13;
/// Formats whose clipboard handle is a GDI object, not an HGLOBAL: they can't be copied as
/// bytes. Windows re-synthesizes CF_BITMAP from CF_DIB, so images still survive.
const GDI_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];

pub struct Snapshot(Vec<(u32, Vec<u8>)>);

/// Opens the clipboard, retrying briefly: another app may hold it for a moment.
fn open() -> bool {
    for _ in 0..20 {
        // SAFETY: plain Win32 call; paired with `CloseClipboard` by the callers.
        if unsafe { OpenClipboard(Some(HWND::default())) }.is_ok() {
            return true;
        }
        sleep(Duration::from_millis(10));
    }
    false
}

fn close() {
    // SAFETY: only called after a successful `open()`.
    let _ = unsafe { CloseClipboard() };
}

pub fn snapshot() -> Option<Snapshot> {
    if !open() {
        return None;
    }
    let mut items = Vec::new();
    let mut format = 0;
    loop {
        // SAFETY: clipboard is open on this thread.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if GDI_FORMATS.contains(&format) || (0x300..=0x3FF).contains(&format) {
            continue;
        }
        // SAFETY: the handle belongs to the clipboard; we only read it while it is open.
        if let Ok(handle) = unsafe { GetClipboardData(format) }
            && let Some(bytes) = read_global(HGLOBAL(handle.0))
        {
            items.push((format, bytes));
        }
    }
    close();
    Some(Snapshot(items))
}

fn read_global(h: HGLOBAL) -> Option<Vec<u8>> {
    // SAFETY: lock/size/unlock on a valid HGLOBAL owned by the clipboard.
    unsafe {
        let size = GlobalSize(h);
        let ptr = GlobalLock(h) as *const u8;
        if ptr.is_null() {
            return None;
        }
        if size == 0 {
            let _ = GlobalUnlock(h);
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(h);
        Some(bytes)
    }
}

fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: fresh movable allocation, filled while locked. Ownership passes to the clipboard
    // on a successful SetClipboardData.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
        let ptr = GlobalLock(h) as *mut u8;
        if ptr.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(h);
        Some(h)
    }
}

fn set_format(format: u32, bytes: &[u8]) {
    if let Some(h) = alloc_global(bytes) {
        // SAFETY: clipboard is open and emptied by us; it takes ownership of `h`.
        let _ = unsafe { SetClipboardData(format, Some(HANDLE(h.0))) };
    }
}

/// Puts `text` on the clipboard, never synced to the cloud clipboard. `transient` also keeps it
/// out of Win+V history and clipboard monitors.
/// Returns the clipboard sequence number after the write.
pub fn write_text(text: &str, transient: bool) -> Option<u32> {
    if !open() {
        return None;
    }
    // SAFETY: clipboard is open on this thread.
    let _ = unsafe { EmptyClipboard() };
    let mut utf16: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    set_format(CF_UNICODETEXT, &utf16);
    // Fully local: nothing Burbuja writes ever syncs to the Windows cloud clipboard, even when
    // it stays in Win+V history.
    // SAFETY: registering well-known format names is side-effect free.
    let cloud = unsafe { RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")) };
    set_format(cloud, &0u32.to_le_bytes());
    if transient {
        // SAFETY: as above.
        let (exclude, history) = unsafe {
            (
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
            )
        };
        set_format(exclude, &[0]);
        set_format(history, &0u32.to_le_bytes());
    }
    utf16.clear();
    close();
    // SAFETY: no preconditions.
    Some(unsafe { GetClipboardSequenceNumber() })
}

/// Restores `snapshot` only if the clipboard still holds what we wrote (sequence unchanged).
pub fn restore_if_unchanged(snapshot: Snapshot, sequence: u32) -> bool {
    // SAFETY: no preconditions.
    if unsafe { GetClipboardSequenceNumber() } != sequence || !open() {
        return false;
    }
    // SAFETY: clipboard is open on this thread.
    let _ = unsafe { EmptyClipboard() };
    for (format, bytes) in &snapshot.0 {
        set_format(*format, bytes);
    }
    close();
    true
}

/// Waits (up to `max`) until no modifier is physically held, so Ctrl+V doesn't become
/// Win+Ctrl+V or Alt+Ctrl+V. Returns false on timeout.
pub fn wait_for_modifiers_released(max: Duration) -> bool {
    const MODS: [VIRTUAL_KEY; 8] = [
        VK_LWIN,
        VK_RWIN,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LSHIFT,
        VK_RSHIFT,
    ];
    let started = Instant::now();
    loop {
        // SAFETY: plain key state query.
        let held = MODS
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0);
        if !held {
            return true;
        }
        if started.elapsed() >= max {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: crate::hotkeys::INJECTED_MARKER,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    // SAFETY: SendInput with fully initialized INPUT structs.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

pub fn send_paste() -> bool {
    send(&[
        key(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, 0, KEYEVENTF_KEYUP),
        key(VK_CONTROL, 0, KEYEVENTF_KEYUP),
    ])
}

/// Types `text` as Unicode key events; `\n` becomes Enter.
pub fn type_text(text: &str) -> bool {
    let mut inputs = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
            }
            _ => {
                let mut buf = [0u16; 2];
                for unit in ch.encode_utf16(&mut buf) {
                    inputs.push(key(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
                    inputs.push(key(
                        VIRTUAL_KEY(0),
                        *unit,
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    ));
                }
            }
        }
    }
    inputs.chunks(64).all(|chunk| {
        let ok = send(chunk);
        sleep(Duration::from_millis(2));
        ok
    })
}

fn process_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    // SAFETY: token handle is closed below; TOKEN_ELEVATION is a plain u32 struct.
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok.then_some(elevation.TokenIsElevated != 0)
    }
}

/// The foreground app's executable name without extension (e.g. `notepad`), for history.
pub fn foreground_app_name() -> Option<String> {
    use windows::Win32::System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW};
    use windows::core::PWSTR;
    // SAFETY: plain Win32 queries; the process handle is closed below.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(process);
        ok.then(|| {
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
        })
        .flatten()
    }
}

/// Puts `text` on the clipboard for "Copy" in the history (not transient).
pub fn copy(text: &str) -> bool {
    write_text(text, false).is_some()
}

/// True when the foreground window belongs to an elevated process and we are not elevated:
/// Windows (UIPI) would silently drop our simulated input.
pub fn foreground_blocks_input() -> bool {
    // SAFETY: plain Win32 queries; the process handle is closed below.
    unsafe {
        if process_elevated(GetCurrentProcess()).unwrap_or(false) {
            return false;
        }
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(process) => {
                // A token we may not query is itself a sign of a higher integrity level.
                let elevated = process_elevated(process).unwrap_or(true);
                let _ = CloseHandle(process);
                elevated
            }
            Err(_) => true,
        }
    }
}
