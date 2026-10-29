//! Win32 details for overlay windows: never activate, stay topmost, exact physical bounds.

use tauri::WebviewWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

fn hwnd(window: &WebviewWindow) -> Option<HWND> {
    window.hwnd().ok()
}

/// `WS_EX_NOACTIVATE`: clicks never activate the window (the text keeps going to the user's
/// app). `WS_EX_TOOLWINDOW`: no taskbar button, not in Alt+Tab.
pub fn make_non_activating(window: &WebviewWindow) {
    let Some(h) = hwnd(window) else { return };
    // SAFETY: reading and writing the extended style of our own window.
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize,
        );
    }
}

/// Shows without activating, then re-asserts topmost (other topmost windows can bury it).
pub fn show_no_activate(window: &WebviewWindow) {
    let Some(h) = hwnd(window) else { return };
    // SAFETY: plain window management on our own HWND.
    unsafe {
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Moves and sizes in one call, in physical pixels of the destination monitor.
pub fn set_bounds(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    let Some(handle) = hwnd(window) else { return };
    // SAFETY: plain window management on our own HWND.
    unsafe {
        let _ = SetWindowPos(handle, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }
}
