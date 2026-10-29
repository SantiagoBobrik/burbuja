//! OS permissions dictation depends on.
//!
//! - macOS gates the microphone, Accessibility (paste + hotkey) and sometimes Input Monitoring
//!   per app, and can show its own prompt the first time.
//! - Windows has no prompt for desktop apps, only the privacy switches in Settings. When one is
//!   off, the microphone still opens but records digital silence, so we read the switches from
//!   the registry to tell the user exactly which one to turn on.

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    InputMonitoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
    /// macOS hasn't asked yet, so it can still show its own prompt.
    NotAsked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Macos,
    Windows,
    Other,
}

/// Which Windows privacy switch blocks the microphone (Settings > Privacy > Microphone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MicBlock {
    /// "Microphone access" for the whole device (set by an administrator).
    Device,
    /// "Let apps access your microphone".
    Apps,
    /// "Let desktop apps access your microphone".
    DesktopApps,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub platform: Platform,
    /// False where the OS has no per-app permissions to guide the user through.
    pub required: bool,
    pub microphone: PermissionState,
    /// Windows only: the switch that is off when `microphone` is denied.
    pub microphone_block: Option<MicBlock>,
    /// At least one input device is connected.
    pub has_microphone: bool,
    pub accessibility: PermissionState,
    pub input_monitoring: PermissionState,
    /// The keyboard hook is running, so the hotkey works.
    pub hotkeys_active: bool,
}

/// What `request` did, so the Hub can say what happens next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PermissionRequest {
    /// The OS showed its own prompt.
    Prompted,
    /// The OS has no prompt left: the caller opens the privacy page.
    NeedsSettings,
}

fn has_microphone() -> bool {
    use cpal::traits::HostTrait;
    cpal::default_host().default_input_device().is_some()
}

/// Microphone permission alone: cheap enough for the hotkey path (no device enumeration).
#[cfg(not(any(target_os = "macos", windows)))]
pub fn microphone_state() -> PermissionState {
    PermissionState::Granted
}

/// False when macOS would drop our paste keystroke.
#[cfg(not(target_os = "macos"))]
pub fn accessibility_granted() -> bool {
    true
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn status() -> Permissions {
    Permissions {
        platform: Platform::Other,
        required: false,
        microphone: PermissionState::Granted,
        microphone_block: None,
        has_microphone: has_microphone(),
        accessibility: PermissionState::Granted,
        input_monitoring: PermissionState::Granted,
        hotkeys_active: true,
    }
}

/// Only macOS keeps per-build approvals that can go stale.
#[cfg(not(target_os = "macos"))]
pub fn reset(kind: PermissionKind, _bundle_id: &str) -> PermissionRequest {
    request(kind)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn request(_: PermissionKind) -> PermissionRequest {
    PermissionRequest::NeedsSettings
}

#[cfg(windows)]
pub use win::{microphone_state, request, status};

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::{HSTRING, PCWSTR, w};

    use super::{MicBlock, PermissionKind, PermissionRequest, PermissionState, Permissions};

    const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    /// True when `{key}\{sub}` has `Value = "Deny"`. A missing key or value means allowed.
    fn denied(root: HKEY, sub: &str) -> bool {
        let path = HSTRING::from(sub);
        let mut buf = [0u16; 16];
        let mut size = std::mem::size_of_val(&buf) as u32;
        // SAFETY: `buf` is writable for `size` bytes; the strings are NUL-terminated.
        let status = unsafe {
            RegGetValueW(
                root,
                PCWSTR(path.as_ptr()),
                w!("Value"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return false;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        String::from_utf16_lossy(&buf[..len]).eq_ignore_ascii_case("deny")
    }

    fn microphone_block() -> Option<MicBlock> {
        if denied(HKEY_LOCAL_MACHINE, CONSENT) {
            Some(MicBlock::Device)
        } else if denied(HKEY_CURRENT_USER, CONSENT) {
            Some(MicBlock::Apps)
        } else if denied(HKEY_CURRENT_USER, &format!("{CONSENT}\\NonPackaged")) {
            Some(MicBlock::DesktopApps)
        } else {
            None
        }
    }

    pub fn microphone_state() -> PermissionState {
        if microphone_block().is_some() {
            PermissionState::Denied
        } else {
            PermissionState::Granted
        }
    }

    pub fn status() -> Permissions {
        let block = microphone_block();
        Permissions {
            platform: super::Platform::Windows,
            required: true,
            microphone: if block.is_some() {
                PermissionState::Denied
            } else {
                PermissionState::Granted
            },
            microphone_block: block,
            has_microphone: super::has_microphone(),
            accessibility: PermissionState::Granted,
            input_monitoring: PermissionState::Granted,
            hotkeys_active: true,
        }
    }

    /// Desktop apps get no prompt on Windows: the switch lives in Settings.
    pub fn request(_: PermissionKind) -> PermissionRequest {
        PermissionRequest::NeedsSettings
    }
}

#[cfg(target_os = "macos")]
pub use macos::{accessibility_granted, microphone as microphone_state, request, reset, status};

/// The FFI below is the documented C / Objective-C API (`AXUIElement.h`, `CGEvent.h`,
/// `CFDictionary.h`, `AVCaptureDevice.h`, `objc/message.h`, clang's Block ABI).
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CString, c_void};
    use std::ptr;

    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{PermissionKind, PermissionRequest, PermissionState, Permissions};

    /// macOS gives no way to know whether the Accessibility / Input Monitoring prompt showed.
    /// The first request of a session counts as the prompt; later ones also open Settings.
    static ASKED_ACCESSIBILITY: AtomicBool = AtomicBool::new(false);
    static ASKED_INPUT_MONITORING: AtomicBool = AtomicBool::new(false);

    type Id = *mut c_void;
    type Sel = *const c_void;

    const AV_NOT_DETERMINED: isize = 0;
    const AV_AUTHORIZED: isize = 3;
    const BLOCK_IS_GLOBAL: i32 = 1 << 28;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void;
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGPreflightListenEventAccess() -> bool;
        fn CGRequestListenEventAccess() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeAudio: Id;
    }

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const std::ffi::c_char) -> Id;
        fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
        fn objc_msgSend();
    }

    unsafe extern "C" {
        static _NSConcreteGlobalBlock: c_void;
    }

    #[repr(C)]
    struct BlockDescriptor {
        reserved: usize,
        size: usize,
    }

    /// A capture-less block literal, as clang lays it out.
    #[repr(C)]
    struct Block {
        isa: *const c_void,
        flags: i32,
        reserved: i32,
        invoke: unsafe extern "C" fn(*const Block, bool),
        descriptor: *const BlockDescriptor,
    }

    /// The Hub polls the status, so the answer itself is not needed.
    unsafe extern "C" fn ignore_answer(_: *const Block, _: bool) {}

    fn capture_device() -> Option<Id> {
        let class = CString::new("AVCaptureDevice").ok()?;
        // SAFETY: a NUL-terminated class name; null when AVFoundation has no such class.
        let device = unsafe { objc_getClass(class.as_ptr()) };
        (!device.is_null()).then_some(device)
    }

    pub fn microphone() -> PermissionState {
        let Some(device) = capture_device() else {
            return PermissionState::Denied;
        };
        let Ok(name) = CString::new("authorizationStatusForMediaType:") else {
            return PermissionState::Denied;
        };
        // SAFETY: the method's documented signature, `+(NSInteger)...:(AVMediaType)`.
        let status = unsafe {
            let send: unsafe extern "C" fn(Id, Sel, Id) -> isize =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            send(device, sel_registerName(name.as_ptr()), AVMediaTypeAudio)
        };
        match status {
            AV_AUTHORIZED => PermissionState::Granted,
            AV_NOT_DETERMINED => PermissionState::NotAsked,
            _ => PermissionState::Denied,
        }
    }

    fn request_microphone() {
        let Some(device) = capture_device() else {
            return;
        };
        let Ok(name) = CString::new("requestAccessForMediaType:completionHandler:") else {
            return;
        };
        // AVFoundation copies the handler and may call it after this returns.
        let descriptor = Box::leak(Box::new(BlockDescriptor {
            reserved: 0,
            size: size_of::<Block>(),
        }));
        let block = Box::leak(Box::new(Block {
            isa: &raw const _NSConcreteGlobalBlock,
            flags: BLOCK_IS_GLOBAL,
            reserved: 0,
            invoke: ignore_answer,
            descriptor,
        }));
        // SAFETY: the method's documented signature, `+(void)...:(AVMediaType)...:(void(^)(BOOL))`.
        unsafe {
            let send: unsafe extern "C" fn(Id, Sel, Id, *const Block) =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            send(
                device,
                sel_registerName(name.as_ptr()),
                AVMediaTypeAudio,
                block,
            );
        }
    }

    fn prompt_accessibility() {
        // SAFETY: a one-entry CFDictionary of CF constants, released after the call.
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
        }
    }

    pub fn accessibility_granted() -> bool {
        // SAFETY: argument-less status query.
        unsafe { AXIsProcessTrusted() }
    }

    fn state(granted: bool) -> PermissionState {
        if granted {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        }
    }

    pub fn status() -> Permissions {
        // SAFETY: argument-less status queries.
        let (accessibility, listen) =
            unsafe { (AXIsProcessTrusted(), CGPreflightListenEventAccess()) };
        Permissions {
            platform: super::Platform::Macos,
            required: true,
            microphone: microphone(),
            microphone_block: None,
            has_microphone: super::has_microphone(),
            accessibility: state(accessibility),
            input_monitoring: state(listen),
            hotkeys_active: crate::hotkeys::tap_active(),
        }
    }

    /// Forgets every approval macOS keeps for this bundle id, then asks again.
    ///
    /// Ad-hoc signed builds are approved by code hash, so after an update System Settings still
    /// shows Burbuja switched on while the new build is refused. Removing the stale entries lets
    /// the prompt add the running build.
    pub fn reset(kind: PermissionKind, bundle_id: &str) -> PermissionRequest {
        let service = match kind {
            PermissionKind::Microphone => "Microphone",
            PermissionKind::Accessibility => "Accessibility",
            PermissionKind::InputMonitoring => "ListenEvent",
        };
        match std::process::Command::new("/usr/bin/tccutil")
            .args(["reset", service, bundle_id])
            .output()
        {
            Ok(out) if out.status.success() => log::info!("reset the {service} approval"),
            Ok(out) => log::warn!(
                "tccutil reset {service} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => log::warn!("tccutil unavailable: {e}"),
        }
        match kind {
            PermissionKind::Accessibility => ASKED_ACCESSIBILITY.store(false, Ordering::SeqCst),
            PermissionKind::InputMonitoring => {
                ASKED_INPUT_MONITORING.store(false, Ordering::SeqCst)
            }
            PermissionKind::Microphone => {}
        }
        request(kind)
    }

    pub fn request(kind: PermissionKind) -> PermissionRequest {
        let prompted = match kind {
            PermissionKind::Microphone => {
                // macOS only prompts once; after a "Don't Allow" the privacy page is the only way.
                let ask = microphone() == PermissionState::NotAsked;
                if ask {
                    request_microphone();
                }
                ask
            }
            PermissionKind::Accessibility => {
                // Always: it (re-)adds this build to the list, which a reset or a new signature
                // leaves out, and shows "Burbuja would like to control this computer".
                prompt_accessibility();
                !ASKED_ACCESSIBILITY.swap(true, Ordering::SeqCst)
            }
            PermissionKind::InputMonitoring => {
                let ask = !ASKED_INPUT_MONITORING.swap(true, Ordering::SeqCst);
                if ask {
                    // SAFETY: argument-less request; it also adds the app to the settings list.
                    unsafe { CGRequestListenEventAccess() };
                }
                ask
            }
        };
        if prompted {
            PermissionRequest::Prompted
        } else {
            PermissionRequest::NeedsSettings
        }
    }
}
