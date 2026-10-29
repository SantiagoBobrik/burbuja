//! Global hotkey service: platform keyboard hook → [`HotkeyMatcher`] → controller.
//!
//! The hook (`win.rs`, `macos.rs`) calls [`KeyHandler::on_key`] for every key transition on
//! its own thread and swallows the key when told to. That call only runs the pure matcher (or
//! the shortcut recorder) and queues what follows; a second thread delivers events to the
//! controller, injects the menu mask and answers "Record new shortcut". The OS removes a hook
//! whose callback is slow, so nothing else happens inside it.

pub mod mac_keys;
pub mod vk;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "macos")]
pub use macos::tap_active;
#[cfg(windows)]
use win as platform;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use crate::core::hotkey_spec::{
    HotkeyEvent, HotkeyMatcher, HotkeyRecorder, HotkeySpec, Key, RecordOutcome,
};

/// Id of a registered binding. Only dictation exists in v2.0; "ask screen" may be added later.
pub type BindingId = u32;
pub const DICTATE: BindingId = 0;

/// `dwExtraInfo` of every key event Burbuja injects (menu mask, paste), so our own hook ignores
/// them. ASCII "BRBJ".
pub const INJECTED_MARKER: usize = 0x4252_424A;

pub type EventSink = Box<dyn Fn(BindingId, HotkeyEvent, Instant) + Send>;
/// Receives the result of "Record new shortcut": `Some(canonical)` or `None` (cancelled).
pub type RecordSink = Box<dyn FnOnce(Option<String>) + Send>;

/// Work queued by the hook callback for the delivery thread.
enum Out {
    Event {
        event: HotkeyEvent,
        at: Instant,
        key: Key,
        down: bool,
    },
    MenuMask,
    Recorded(RecordSink, Option<String>),
}

/// State the hook callback works on.
pub(crate) struct KeyHandler {
    matcher: Mutex<HotkeyMatcher>,
    recording: Mutex<Option<(HotkeyRecorder, RecordSink)>>,
    out: Sender<Out>,
}

/// A panic elsewhere must not take the keyboard hook down with it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl KeyHandler {
    /// Called by the platform hook for each key transition. Returns true to swallow the key.
    pub(crate) fn on_key(&self, key: Key, down: bool, at: Instant) -> bool {
        {
            let mut rec = lock(&self.recording);
            if let Some((recorder, _)) = rec.as_mut() {
                if let Some(outcome) = recorder.feed(key, down)
                    && let Some((_, on_done)) = rec.take()
                {
                    let result = match outcome {
                        RecordOutcome::Recorded(h) => Some(h),
                        RecordOutcome::Cancelled => None,
                    };
                    let _ = self.out.send(Out::Recorded(on_done, result));
                }
                return false;
            }
        }
        let mut events = Vec::new();
        let (swallow, mask) = {
            let mut m = lock(&self.matcher);
            let swallow = m.process(key, down, &mut events);
            (swallow, m.spec().needs_menu_mask())
        };
        for event in events {
            if mask && event == HotkeyEvent::Pressed {
                let _ = self.out.send(Out::MenuMask);
            }
            let _ = self.out.send(Out::Event {
                event,
                at,
                key,
                down,
            });
        }
        swallow
    }
}

pub struct HotkeyService {
    handler: Arc<KeyHandler>,
}

impl HotkeyService {
    /// Installs the keyboard hook. Fails when the OS refuses it (e.g. macOS permissions).
    pub fn spawn(spec: HotkeySpec, sink: EventSink) -> Result<Self, String> {
        let (out, rx) = mpsc::channel();
        let handler = Arc::new(KeyHandler {
            matcher: Mutex::new(HotkeyMatcher::with_physical_state(
                spec,
                platform::is_key_down,
            )),
            recording: Mutex::new(None),
            out,
        });
        thread::Builder::new()
            .name("burbuja-hotkey-events".into())
            .spawn(move || deliver(rx, sink))
            .map_err(|e| e.to_string())?;
        platform::install(Arc::clone(&handler))?;
        Ok(Self { handler })
    }

    pub fn set_spec(&self, spec: HotkeySpec) {
        lock(&self.handler.matcher).set_spec(spec);
    }

    /// The next key combination is captured instead of triggering dictation.
    pub fn start_recording(&self, on_done: RecordSink) {
        let previous = lock(&self.handler.recording).replace((HotkeyRecorder::default(), on_done));
        if let Some((_, previous)) = previous {
            previous(None);
        }
    }

    pub fn cancel_recording(&self) {
        let current = lock(&self.handler.recording).take();
        if let Some((_, on_done)) = current {
            on_done(None);
        }
    }
}

fn deliver(rx: Receiver<Out>, sink: EventSink) {
    for out in rx {
        match out {
            Out::Event {
                event,
                at,
                key,
                down,
            } => {
                // Only hotkey events, and the key only when it is a modifier or Esc: the log
                // must never become a keystroke record.
                let cause = if matches!(key, Key::Main(_)) {
                    None
                } else {
                    Some(key)
                };
                log::debug!("hotkey {event:?} cause={cause:?} down={down}");
                sink(DICTATE, event, at);
            }
            Out::MenuMask => platform::send_menu_mask(),
            Out::Recorded(on_done, result) => on_done(result),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn install(_: Arc<KeyHandler>) -> Result<(), String> {
        Err("global hotkeys are not supported on this platform".into())
    }

    pub(super) fn is_key_down(_: Key) -> bool {
        true
    }

    pub(super) fn send_menu_mask() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hotkey_spec::ModKey;

    fn handler(spec: &str) -> (KeyHandler, Receiver<Out>) {
        let (out, rx) = mpsc::channel();
        let handler = KeyHandler {
            matcher: Mutex::new(HotkeyMatcher::new(HotkeySpec::parse(spec).unwrap())),
            recording: Mutex::new(None),
            out,
        };
        (handler, rx)
    }

    fn events(rx: &Receiver<Out>) -> Vec<String> {
        rx.try_iter()
            .map(|o| match o {
                Out::Event { event, .. } => format!("{event:?}"),
                Out::MenuMask => "Mask".into(),
                Out::Recorded(_, r) => format!("Recorded({r:?})"),
            })
            .collect()
    }

    const CTRL: Key = Key::Mod(ModKey::LeftCtrl);
    const WIN: Key = Key::Mod(ModKey::LeftSuper);
    const SHIFT: Key = Key::Mod(ModKey::LeftShift);

    #[test]
    fn modifier_only_hotkey_is_never_swallowed_and_masks_win() {
        let (h, rx) = handler("Ctrl+Super");
        let now = Instant::now();
        assert!(!h.on_key(CTRL, true, now));
        assert!(!h.on_key(WIN, true, now));
        assert!(!h.on_key(WIN, false, now));
        assert!(!h.on_key(CTRL, false, now));
        assert_eq!(events(&rx), ["Mask", "Pressed", "Released"]);
    }

    #[test]
    fn esc_is_swallowed_only_while_the_hotkey_is_held() {
        let (h, rx) = handler("Ctrl+Super");
        let now = Instant::now();
        assert!(!h.on_key(Key::Escape, true, now));
        assert!(!h.on_key(Key::Escape, false, now));
        h.on_key(CTRL, true, now);
        h.on_key(WIN, true, now);
        assert!(h.on_key(Key::Escape, true, now));
        assert_eq!(events(&rx), ["Escape", "Mask", "Pressed", "Escape"]);
    }

    #[test]
    fn combo_swallows_its_main_key_but_not_the_modifiers() {
        let (h, rx) = handler("Ctrl+Shift+Space");
        let now = Instant::now();
        let space = Key::Main("Space");
        assert!(!h.on_key(CTRL, true, now));
        assert!(!h.on_key(SHIFT, true, now));
        assert!(h.on_key(space, true, now));
        assert!(h.on_key(space, true, now), "autorepeat");
        assert!(h.on_key(space, false, now));
        assert_eq!(events(&rx), ["Pressed", "Released"]);
        // Space without the modifiers types normally.
        h.on_key(SHIFT, false, now);
        h.on_key(CTRL, false, now);
        assert!(!h.on_key(space, true, now));
    }

    #[test]
    fn recording_captures_keys_instead_of_dictating() {
        let (h, rx) = handler("Ctrl+Super");
        let (done_tx, done_rx) = mpsc::channel();
        *lock(&h.recording) = Some((
            HotkeyRecorder::default(),
            Box::new(move |r| done_tx.send(r).unwrap()),
        ));
        let now = Instant::now();
        let alt = Key::Mod(ModKey::RightAlt);
        assert!(!h.on_key(alt, true, now));
        assert!(!h.on_key(alt, false, now));
        match rx.try_recv() {
            Ok(Out::Recorded(on_done, result)) => on_done(result),
            _ => panic!("expected a recording result"),
        }
        assert_eq!(done_rx.try_recv().unwrap().as_deref(), Some("RightAlt"));
        assert!(lock(&h.recording).is_none());
    }
}
