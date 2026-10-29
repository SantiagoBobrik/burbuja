//! System tray: monochrome bubble, recording / off states, left click opens the Hub.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::settings::UiLanguage;

const ICON_NORMAL: &[u8] = include_bytes!("../icons/tray/tray-normal.png");
const ICON_RECORDING: &[u8] = include_bytes!("../icons/tray/tray-recording.png");
const ICON_OFF: &[u8] = include_bytes!("../icons/tray/tray-off.png");

pub const ID_ENABLED: &str = "tray-enabled";
pub const ID_OPEN: &str = "tray-open";
pub const ID_HIDE: &str = "tray-hide-bubble";
pub const ID_QUIT: &str = "tray-quit";

/// Whether dictation is on (the tray's "Enabled" toggle). Hotkeys are ignored when off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

pub struct Tray {
    icon: TrayIcon<Wry>,
    enabled_item: CheckMenuItem<Wry>,
}

struct Labels {
    status: String,
    enabled: &'static str,
    open: &'static str,
    hide: &'static str,
    quit: &'static str,
}

fn labels(spanish: bool, hotkey: &str) -> Labels {
    if spanish {
        Labels {
            status: format!("Mantén {hotkey} para dictar"),
            enabled: "Activado",
            open: "Abrir Burbuja",
            hide: "Ocultar la burbuja 1 hora",
            quit: "Salir",
        }
    } else {
        Labels {
            status: format!("Hold {hotkey} to dictate"),
            enabled: "Enabled",
            open: "Open Burbuja",
            hide: "Hide bubble for 1 hour",
            quit: "Quit",
        }
    }
}

/// Resolves the UI language like the frontend does: `system` follows the OS locale.
pub fn is_spanish(pref: UiLanguage) -> bool {
    match pref {
        UiLanguage::Es => true,
        UiLanguage::En => false,
        UiLanguage::System => system_locale().to_lowercase().starts_with("es"),
    }
}

#[cfg(windows)]
fn system_locale() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH wide.
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    String::from_utf16_lossy(&buf[..(len.max(1) as usize - 1)])
}

#[cfg(not(windows))]
fn system_locale() -> String {
    std::env::var("LANG").unwrap_or_default()
}

pub fn create(app: &AppHandle, spanish: bool, hotkey_label: &str) -> tauri::Result<Tray> {
    let l = labels(spanish, hotkey_label);
    let status = MenuItem::with_id(app, "tray-status", l.status, false, None::<&str>)?;
    let enabled_item =
        CheckMenuItem::with_id(app, ID_ENABLED, l.enabled, true, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &enabled_item,
            &MenuItem::with_id(app, ID_OPEN, l.open, true, None::<&str>)?,
            &MenuItem::with_id(app, ID_HIDE, l.hide, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, ID_QUIT, l.quit, true, None::<&str>)?,
        ],
    )?;
    let icon = TrayIconBuilder::with_id("burbuja")
        .icon(Image::from_bytes(ICON_NORMAL)?)
        .tooltip("Burbuja")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::overlay::open_hub(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(Tray { icon, enabled_item })
}

impl Tray {
    pub fn set_recording(&self, recording: bool) {
        let bytes = if !ENABLED.load(Ordering::SeqCst) {
            ICON_OFF
        } else if recording {
            ICON_RECORDING
        } else {
            ICON_NORMAL
        };
        if let Ok(image) = Image::from_bytes(bytes) {
            let _ = self.icon.set_icon(Some(image));
        }
    }

    pub fn toggle_enabled(&self) {
        let enabled = !ENABLED.load(Ordering::SeqCst);
        ENABLED.store(enabled, Ordering::SeqCst);
        let _ = self.enabled_item.set_checked(enabled);
        self.set_recording(false);
    }
}

/// Handles tray menu clicks; other menus' ids fall through.
pub fn on_menu(app: &AppHandle, id: &str) {
    match id {
        ID_ENABLED => {
            if let Some(tray) = app.try_state::<Tray>() {
                tray.toggle_enabled();
            }
        }
        ID_OPEN => crate::overlay::open_hub(app),
        ID_HIDE => crate::overlay::hide_idle_for_an_hour(app),
        ID_QUIT => app.exit(0),
        _ => {}
    }
}
