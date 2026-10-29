//! Typed events, Rust → frontend. Names are the kebab-case struct names.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UiState {
    Idle,
    Listening,
    HandsFree,
    Processing,
    /// LLM cleanup in progress (Esc still cancels).
    Polishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RecordMode {
    Hold,
    HandsFree,
}

/// Why the state went back to idle without a result, when the UI should say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IdleReason {
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DictationState {
    pub state: UiState,
    pub mode: Option<RecordMode>,
    /// Unix epoch milliseconds when recording started.
    pub started_at: Option<f64>,
    pub elapsed_ms: u32,
    pub limit_ms: u32,
    pub reason: Option<IdleReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MicLevel {
    pub levels: Vec<f32>,
    /// Highest raw sample since the stream opened (settings diagnostic).
    pub peak: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DictationResult {
    pub id: u32,
    pub text: String,
    pub raw: String,
    pub duration_ms: u32,
    pub engine: String,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MicSilent,
    MicUnavailable,
    /// The OS blocks the microphone for Burbuja (privacy setting).
    MicPermission,
    /// No input device is connected.
    NoMicrophone,
    /// macOS dropped the paste: Accessibility is off. The text is on the clipboard.
    AccessibilityMissing,
    ModelMissing,
    EngineCrashed,
    ProviderError,
    ElevatedTarget,
    InputBlocked,
    TooShort,
    NoSpeech,
    /// The local LLM server (Ollama / LM Studio) could not be reached.
    Offline,
    /// The local LLM server took longer than the cleanup timeout.
    ServiceTimeout,
    /// Text inserted, but the AI cleanup failed (info, not an error).
    CleanupSkipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DictationError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SettingsChanged;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct HistoryChanged;

/// Result of "Record new shortcut": the canonical hotkey, or `None` if cancelled.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyRecorded {
    pub hotkey: Option<String>,
    /// A known clash with a system shortcut, if any (shown as a warning).
    pub conflict: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadProgress {
    pub id: String,
    pub bytes: f64,
    pub total: f64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Downloading,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct EngineStatusChanged(pub crate::engine::EngineStatus);

/// Where the time of the last dictation went (Advanced → "Last dictation timings").
/// Stage durations in milliseconds; `None` when a stage did not run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DictationTimings {
    pub hotkey_to_record_ms: f64,
    pub release_to_stop_ms: f64,
    pub audio_ms: f64,
    /// Voice activity detection (silence trimming) in the engine.
    pub vad_ms: f64,
    pub transcribe_ms: f64,
    pub cleanup_ms: Option<f64>,
    pub insert_ms: f64,
    /// Key release → text inserted.
    pub total_ms: f64,
    pub engine: String,
}

/// A dictation failed because of a missing OS permission: the Hub opens on the permissions page.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsNeeded {
    pub kind: crate::permissions::PermissionKind,
}
