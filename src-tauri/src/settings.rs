//! User settings, persisted as top-level keys in `settings.json` via
//! tauri-plugin-store. Unknown or missing fields fall back to defaults per field.
//!
//! Burbuja is fully local: there is no cloud transcription, and the AI
//! cleanup only talks to a server on this computer (Ollama / LM Studio on a loopback address).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};
use tauri_plugin_store::{Store, StoreExt};

use crate::core::hotkey_spec::HotkeySpec;
use crate::core::text::Replacement;

const STORE_FILE: &str = "settings.json";
pub const VERSION: u32 = 1;

/// Keys that earlier builds stored and this one no longer reads (cloud transcription). They are
/// dropped from the store at startup and on every save.
const REMOVED_KEYS: &[&str] = &[
    "transcriber",
    "fallback_to_local",
    "groq_model",
    "openai_transcribe_model",
];

// The cloud choices of earlier builds ("auto" and three cloud providers) no longer deserialize,
// so `from_entries` falls back to the default, `off`: a local server has to be picked explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum CleanupProvider {
    Off,
    Ollama,
    Lmstudio,
}

/// How the AI cleanup formats the text. The presets only change capitalization and punctuation,
/// never the words; `Custom` sends the user's own prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStyle {
    #[default]
    Standard,
    Formal,
    Casual,
    VeryCasual,
    Excited,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum InsertMethod {
    Paste,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    System,
    En,
    Es,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub hotkey: String,
    pub double_tap_window_ms: u32,
    pub max_session_minutes: u32,
    pub microphone: String,
    pub active_model: String,
    pub use_gpu: bool,
    pub language: String,
    pub unload_after_idle_minutes: u32,
    pub cleanup: CleanupProvider,
    pub cleanup_model: String,
    /// Empty = the provider's default local address. Must be a loopback URL.
    pub cleanup_base_url: String,
    pub cleanup_timeout_seconds: u32,
    pub cleanup_style: CleanupStyle,
    /// The user's own cleanup prompt for `CleanupStyle::Custom`; `${output}` marks where the
    /// transcript goes.
    pub cleanup_custom_prompt: String,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub insert_method: InsertMethod,
    pub restore_clipboard: bool,
    pub show_idle_bubble: bool,
    pub sounds_enabled: bool,
    pub sound_volume: f32,
    pub history_retention_days: u32,
    pub keep_audio: bool,
    pub ui_language: UiLanguage,
    pub onboarding_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            hotkey: default_hotkey().into(),
            double_tap_window_ms: 300,
            max_session_minutes: 5,
            microphone: String::new(),
            active_model: "whisper-large-v3-turbo".into(),
            use_gpu: true,
            language: "auto".into(),
            unload_after_idle_minutes: 30,
            // Stays `off`: removed cloud choices of earlier builds load as the default.
            cleanup: CleanupProvider::Off,
            cleanup_model: String::new(),
            cleanup_base_url: String::new(),
            cleanup_timeout_seconds: 15,
            cleanup_style: CleanupStyle::Standard,
            cleanup_custom_prompt: String::new(),
            vocabulary: Vec::new(),
            replacements: Vec::new(),
            insert_method: InsertMethod::Paste,
            restore_clipboard: false,
            show_idle_bubble: true,
            sounds_enabled: true,
            sound_volume: 0.6,
            history_retention_days: 30,
            keep_audio: false,
            ui_language: UiLanguage::System,
            onboarding_completed: false,
        }
    }
}

/// Reads the settings from `settings.json` (defaults if the store can't be opened). Settings of
/// features removed in earlier versions are dropped from the file, which also rewrites
/// cleanup choices that now load as "off".
pub fn load<R: Runtime>(app: &impl Manager<R>) -> Settings {
    let Ok(store) = app.store(STORE_FILE) else {
        return Settings::default();
    };
    let settings = Settings::from_entries(store.entries());
    if REMOVED_KEYS.iter().any(|k| store.has(k))
        && let Err(e) = write(&store, &settings)
    {
        log::warn!("could not migrate settings: {e}");
    }
    settings
}

/// Writes `settings` to `settings.json`. The caller validates first.
pub fn save<R: Runtime>(app: &impl Manager<R>, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    write(&store, settings).map_err(|e| e.to_string())
}

fn write<R: Runtime>(store: &Store<R>, settings: &Settings) -> tauri_plugin_store::Result<()> {
    for (key, value) in settings.to_entries() {
        store.set(key, value);
    }
    for key in REMOVED_KEYS {
        store.delete(key);
    }
    store.save()
}

pub fn default_hotkey() -> &'static str {
    if cfg!(target_os = "macos") {
        "Fn"
    } else {
        "Ctrl+Super"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Type)]
#[serde(tag = "field", content = "message", rename_all = "camelCase")]
pub enum SettingsError {
    #[error("hotkey: {0}")]
    Hotkey(String),
    #[error("{0} is out of range")]
    Range(String),
    #[error("{0} is too long")]
    TooLong(String),
    #[error("the AI cleanup server must be on this computer (localhost, 127.0.0.1 or [::1])")]
    NotLocal,
}

/// True for an `http(s)` URL whose host is a loopback address (`localhost`, `127.0.0.0/8`,
/// `[::1]`), so cleanup requests can never leave the device.
pub fn is_loopback_url(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

const MAX_TEXT: usize = 512;
/// A custom cleanup prompt can be a few paragraphs.
const MAX_PROMPT: usize = 4000;
const MAX_LIST: usize = 1000;

impl Settings {
    /// Validates user input from the UI (validate every command input).
    pub fn validate(&self) -> Result<(), SettingsError> {
        HotkeySpec::parse(&self.hotkey).map_err(|e| SettingsError::Hotkey(e.to_string()))?;
        let range = |ok: bool, name: &str| {
            if ok {
                Ok(())
            } else {
                Err(SettingsError::Range(name.into()))
            }
        };
        range(
            (150..=800).contains(&self.double_tap_window_ms),
            "double_tap_window_ms",
        )?;
        range(
            (1..=60).contains(&self.max_session_minutes),
            "max_session_minutes",
        )?;
        range(
            self.unload_after_idle_minutes <= 24 * 60,
            "unload_after_idle_minutes",
        )?;
        range(
            (1..=120).contains(&self.cleanup_timeout_seconds),
            "cleanup_timeout_seconds",
        )?;
        range((0.0..=1.0).contains(&self.sound_volume), "sound_volume")?;
        range(
            [0, 7, 30, 90].contains(&self.history_retention_days),
            "history_retention_days",
        )?;
        let texts = [
            ("microphone", &self.microphone),
            ("active_model", &self.active_model),
            ("language", &self.language),
            ("cleanup_model", &self.cleanup_model),
            ("cleanup_base_url", &self.cleanup_base_url),
        ];
        for (name, value) in texts {
            if value.len() > MAX_TEXT {
                return Err(SettingsError::TooLong(name.into()));
            }
        }
        if !self.cleanup_base_url.trim().is_empty() && !is_loopback_url(&self.cleanup_base_url) {
            return Err(SettingsError::NotLocal);
        }
        if self.cleanup_custom_prompt.len() > MAX_PROMPT {
            return Err(SettingsError::TooLong("cleanup_custom_prompt".into()));
        }
        if self.vocabulary.len() > MAX_LIST || self.vocabulary.iter().any(|v| v.len() > MAX_TEXT) {
            return Err(SettingsError::TooLong("vocabulary".into()));
        }
        if self.replacements.len() > MAX_LIST
            || self
                .replacements
                .iter()
                .any(|r| r.from.len() > MAX_TEXT || r.to.len() > MAX_TEXT)
        {
            return Err(SettingsError::TooLong("replacements".into()));
        }
        Ok(())
    }

    /// Reads settings from the store's key/value pairs, keeping defaults for anything missing
    /// or invalid. Keys of removed settings are ignored.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, serde_json::Value)>) -> Self {
        let mut map: serde_json::Map<String, serde_json::Value> = entries.into_iter().collect();
        let defaults = serde_json::to_value(Settings::default()).expect("defaults serialize");
        // Drop fields that don't deserialize on their own, so one bad value can't reset the rest.
        map.retain(|k, v| {
            let mut probe = defaults.clone();
            probe[k.as_str()] = v.clone();
            serde_json::from_value::<Settings>(probe).is_ok()
        });
        let mut settings: Settings =
            serde_json::from_value(serde_json::Value::Object(map)).unwrap_or_default();
        // An earlier build may have stored a remote (cloud) cleanup URL: forget just that one.
        if !settings.cleanup_base_url.trim().is_empty()
            && !is_loopback_url(&settings.cleanup_base_url)
        {
            settings.cleanup_base_url.clear();
        }
        if settings.validate().is_ok() {
            settings
        } else {
            Settings::default()
        }
    }

    pub fn to_entries(&self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, default_hotkey());
        assert_eq!(s.double_tap_window_ms, 300);
        assert_eq!(s.sound_volume, 0.6);
        assert!(s.validate().is_ok());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["cleanup"], "off");
        assert_eq!(v["insert_method"], "paste");
        assert_eq!(v["ui_language"], "system");
    }

    #[test]
    fn missing_and_bad_fields_fall_back_per_field() {
        let s = Settings::from_entries([
            ("microphone".to_string(), json!("HyperX SoloCast")),
            ("cleanup".to_string(), json!("bogus")),
            ("sound_volume".to_string(), json!("loud")),
        ]);
        assert_eq!(s.microphone, "HyperX SoloCast");
        assert_eq!(s.cleanup, CleanupProvider::Off);
        assert_eq!(s.sound_volume, 0.6);
    }

    #[test]
    fn settings_from_cloud_builds_load_as_local_only() {
        for old in ["auto", "anthropic", "openai", "groq"] {
            let s = Settings::from_entries([
                ("transcriber".to_string(), json!("groq")),
                ("fallback_to_local".to_string(), json!(false)),
                ("groq_model".to_string(), json!("whisper-large-v3")),
                ("openai_transcribe_model".to_string(), json!("whisper-1")),
                ("cleanup".to_string(), json!(old)),
                (
                    "cleanup_base_url".to_string(),
                    json!("https://api.openai.com/v1"),
                ),
                ("microphone".to_string(), json!("Mic")),
            ]);
            assert_eq!(s.cleanup, CleanupProvider::Off, "{old}");
            assert_eq!(s.cleanup_base_url, "");
            assert_eq!(s.microphone, "Mic", "other settings survive");
            let entries = s.to_entries();
            assert!(REMOVED_KEYS.iter().all(|k| !entries.contains_key(*k)));
        }
        let s = Settings::from_entries([
            ("cleanup".to_string(), json!("ollama")),
            (
                "cleanup_base_url".to_string(),
                json!("http://127.0.0.1:11434/v1"),
            ),
        ]);
        assert_eq!(s.cleanup, CleanupProvider::Ollama);
        assert_eq!(s.cleanup_base_url, "http://127.0.0.1:11434/v1");
    }

    #[test]
    fn cleanup_base_url_must_be_loopback() {
        for ok in [
            "http://localhost:11434/v1",
            "http://LOCALHOST:1234/v1/",
            "http://127.0.0.1:1234/v1",
            "https://127.0.0.2/v1",
            "http://[::1]:8080/v1",
            // The URL parser normalizes numeric IPv4 forms; these really are 127.0.0.1.
            "http://2130706433:1234/v1",
            "http://0x7f.1:1234/v1",
        ] {
            assert!(is_loopback_url(ok), "{ok}");
        }
        for bad in [
            "https://api.openai.com/v1",
            "http://box:11434/v1",
            "http://192.168.1.10:11434/v1",
            "http://localhost.example.com/v1",
            "http://127.0.0.1.nip.io/v1",
            "http://user@localhost:11434/v1",
            "file:///etc/passwd",
            "localhost:11434",
            "",
            // Not loopback: "any address" binds, mapped/compat forms, look-alike names.
            "http://0.0.0.0:11434/v1",
            "http://[::]:11434/v1",
            "http://[::ffff:127.0.0.1]:11434/v1",
            "http://localhost.:11434/v1",
            "http://localhost%2eevil.com/v1",
            "http://evil.com#@localhost/v1",
            "http://localhost@evil.com/v1",
            "ftp://localhost/v1",
        ] {
            assert!(!is_loopback_url(bad), "{bad}");
        }
        let mut s = Settings {
            cleanup_base_url: "http://10.0.0.5:11434/v1".into(),
            ..Settings::default()
        };
        assert_eq!(s.validate(), Err(SettingsError::NotLocal));
        s.cleanup_base_url = "http://localhost:11434/v1".into();
        assert!(s.validate().is_ok());
        s.cleanup_base_url = String::new();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let mut s = Settings {
            double_tap_window_ms: 100,
            ..Settings::default()
        };
        assert_eq!(
            s.validate(),
            Err(SettingsError::Range("double_tap_window_ms".into()))
        );
        s.double_tap_window_ms = 300;
        s.hotkey = "ctrl+bogus".into();
        assert!(matches!(s.validate(), Err(SettingsError::Hotkey(_))));
    }

    #[test]
    fn entries_round_trip() {
        let s = Settings {
            microphone: "Mic".into(),
            sound_volume: 0.3,
            ..Settings::default()
        };
        assert_eq!(Settings::from_entries(s.to_entries()), s);
    }
}
