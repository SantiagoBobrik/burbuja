//! Non-activating overlay windows. Generic: the bubble today, the
//! future answer panel and draw canvas later, with the same guarantees (never focused,
//! always on top, on the monitor under the mouse).

#[cfg(windows)]
mod windows;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_specta::Event;

pub const BUBBLE: &str = "overlay";

/// Fixed logical canvas for the bubble: wide enough for the widest message (360) plus spring
/// overshoot. The pill animates inside it; the native window never resizes per state.
const BUBBLE_W: f64 = 400.0;
const BUBBLE_H: f64 = 56.0;
/// Gap above the taskbar / Dock.
const BOTTOM_GAP: f64 = 16.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct PillBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct OverlayHover {
    pub hovered: bool,
}

/// Shared overlay state: what part of the canvas is the pill (in logical px), and whether the
/// window is visible (the hover watcher sleeps otherwise).
#[derive(Default)]
pub struct OverlayState {
    pub pill: Mutex<PillBounds>,
    pub visible: AtomicBool,
    /// "Hide for 1 hour": the idle bubble stays hidden until then (dictation still shows it).
    pub idle_hidden_until: Mutex<Option<std::time::Instant>>,
}

impl OverlayState {
    pub fn idle_hidden(&self) -> bool {
        self.idle_hidden_until
            .lock()
            .expect("hidden until")
            .is_some_and(|t| std::time::Instant::now() < t)
    }
}

pub const MENU_HIDE_HOUR: &str = "bubble-hide-hour";
pub const MENU_SETTINGS: &str = "bubble-settings";

/// Native context menu for a right click on the bubble.
pub fn popup_menu(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    let Some(window) = app.get_webview_window(BUBBLE) else {
        return Ok(());
    };
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, MENU_HIDE_HOUR, "Hide for 1 hour", true, None::<&str>)?,
            &MenuItem::with_id(app, MENU_SETTINGS, "Settings", true, None::<&str>)?,
        ],
    )?;
    window.popup_menu(&menu)
}

pub fn hide_idle_for_an_hour(app: &AppHandle) {
    let state = app.state::<Arc<OverlayState>>();
    *state.idle_hidden_until.lock().expect("hidden until") =
        Some(std::time::Instant::now() + Duration::from_secs(3600));
    hide_bubble(app);
}

pub fn open_hub(app: &AppHandle) {
    if let Some(hub) = app.get_webview_window("hub") {
        let _ = hub.unminimize();
        let _ = hub.show();
        let _ = hub.set_focus();
    }
}

pub fn create_bubble(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, BUBBLE, WebviewUrl::App("overlay.html".into()))
        .title("Burbuja bubble")
        .inner_size(BUBBLE_W, BUBBLE_H)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focusable(false)
        .focused(false)
        .accept_first_mouse(true)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .visible(false)
        .build()?;
    #[cfg(windows)]
    windows::make_non_activating(&window);
    let _ = window.set_ignore_cursor_events(true);
    Ok(window)
}

/// Shows the bubble on the monitor under the mouse without activating it.
pub fn show_bubble(app: &AppHandle) {
    let Some(window) = app.get_webview_window(BUBBLE) else {
        return;
    };
    place(app, &window);
    #[cfg(windows)]
    windows::show_no_activate(&window);
    #[cfg(not(windows))]
    let _ = window.show();
    app.state::<Arc<OverlayState>>()
        .visible
        .store(true, Ordering::SeqCst);
}

pub fn hide_bubble(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(BUBBLE) {
        let _ = window.hide();
        let _ = window.set_ignore_cursor_events(true);
    }
    app.state::<Arc<OverlayState>>()
        .visible
        .store(false, Ordering::SeqCst);
}

/// Bottom center of the work area of the monitor under the cursor, 16 px above the taskbar,
/// computed in that monitor's physical pixels so cross-DPI moves land right.
fn place(app: &AppHandle, window: &WebviewWindow) {
    let cursor = app.cursor_position().ok();
    let monitors = app.available_monitors().unwrap_or_default();
    let monitor = cursor
        .and_then(|c| {
            monitors.iter().find(|m| {
                let (p, s) = (m.position(), m.size());
                c.x >= f64::from(p.x)
                    && c.x < f64::from(p.x) + f64::from(s.width)
                    && c.y >= f64::from(p.y)
                    && c.y < f64::from(p.y) + f64::from(s.height)
            })
        })
        .cloned()
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let (w, h) = (
        (BUBBLE_W * scale).round() as i32,
        (BUBBLE_H * scale).round() as i32,
    );
    let x = work.position.x + (work.size.width as i32 - w) / 2;
    let y = work.position.y + work.size.height as i32 - h - (BOTTOM_GAP * scale).round() as i32;
    #[cfg(windows)]
    windows::set_bounds(window, x, y, w, h);
    #[cfg(not(windows))]
    {
        let _ = window.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// Lets the mouse through everywhere except over the visible pill, and tells the
/// bubble when it's hovered. Polls the cursor at 30 Hz only while the bubble is visible.
pub fn spawn_hover_watch(app: AppHandle, state: Arc<OverlayState>) {
    thread::Builder::new()
        .name("burbuja-overlay-hover".into())
        .spawn(move || {
            let mut hovered = false;
            loop {
                if !state.visible.load(Ordering::SeqCst) {
                    if hovered {
                        hovered = false;
                        let _ = OverlayHover { hovered }.emit_to(&app, BUBBLE);
                    }
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }
                thread::sleep(Duration::from_millis(33));
                let Some(window) = app.get_webview_window(BUBBLE) else {
                    continue;
                };
                let (Ok(cursor), Ok(origin), Ok(scale)) = (
                    app.cursor_position(),
                    window.outer_position(),
                    window.scale_factor(),
                ) else {
                    continue;
                };
                let pill = *state.pill.lock().expect("pill bounds");
                // A little slack so the hover doesn't flicker at the edge.
                let margin = 4.0;
                let (lx, ly) = (
                    (cursor.x - f64::from(origin.x)) / scale,
                    (cursor.y - f64::from(origin.y)) / scale,
                );
                let inside = pill.width > 0.0
                    && lx >= pill.x - margin
                    && lx <= pill.x + pill.width + margin
                    && ly >= pill.y - margin
                    && ly <= pill.y + pill.height + margin;
                if inside != hovered {
                    hovered = inside;
                    let _ = window.set_ignore_cursor_events(!inside);
                    let _ = OverlayHover { hovered }.emit_to(&app, BUBBLE);
                }
            }
        })
        .expect("spawn overlay hover thread");
}
