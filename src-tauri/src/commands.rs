//! Every Tauri command, typed with specta. Commands validate input and dispatch;
//! business logic lives in the services.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::audio::{self, Mic, MicInfo};
use crate::cleanup::{self, Cleanup};
use crate::core::hotkey_spec::{self, HotkeySpec};
use crate::dictation::{ControllerHandle, Msg};
use crate::engine::{EngineConfig, EngineHost, EngineStatus, models};
use crate::events::{DownloadStatus, MicLevel, ModelDownloadProgress, SettingsChanged};
use crate::events::{HistoryChanged, HotkeyRecorded};
use crate::history::{self, History, HistoryItem, Stats};
use crate::hotkeys::HotkeyService;
use crate::insert;
use crate::overlay::{self, OverlayState, PillBounds};
use crate::permissions::{self, PermissionKind, PermissionRequest, Permissions};
use crate::settings::{self, Settings, SettingsError};
use crate::sound::{SoundKind, SoundPlayer};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub controller: ControllerHandle,
    pub sounds: SoundPlayer,
    pub hotkeys: Option<HotkeyService>,
    pub preview: Mutex<Option<Mic>>,
    pub engine: Arc<EngineHost>,
    pub downloads: models::Downloads,
    pub history: Arc<History>,
}

#[derive(Debug, Clone, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum CommandError {
    #[error("invalid settings: {0}")]
    InvalidSettings(SettingsError),
    #[error("microphone: {0}")]
    Microphone(String),
    #[error("could not save settings: {0}")]
    Storage(String),
    #[error("model: {0}")]
    Model(String),
    #[error("history: {0}")]
    History(String),
    #[error("hotkey: {0}")]
    Hotkey(String),
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().expect("settings").clone()
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, CommandError> {
    apply_settings(&app, &state, settings)
}

/// Validates, persists and propagates settings to every service.
fn apply_settings(
    app: &AppHandle,
    state: &AppState,
    settings: Settings,
) -> Result<Settings, CommandError> {
    settings.validate().map_err(CommandError::InvalidSettings)?;
    if models::find(&settings.active_model).is_none() {
        return Err(CommandError::InvalidSettings(SettingsError::Range(
            "active_model".into(),
        )));
    }
    settings::save(app, &settings).map_err(CommandError::Storage)?;

    let previous = std::mem::replace(
        &mut *state.settings.lock().expect("settings"),
        settings.clone(),
    );
    if previous.hotkey != settings.hotkey
        && let (Some(hotkeys), Ok(spec)) = (&state.hotkeys, HotkeySpec::parse(&settings.hotkey))
    {
        hotkeys.set_spec(spec);
    }
    if previous.sound_volume != settings.sound_volume {
        state.sounds.set_volume(settings.sound_volume);
    }
    state.engine.configure(engine_config(app, &settings));
    state
        .controller
        .send(Msg::Settings(Box::new(settings.clone())));
    let _ = SettingsChanged.emit(app);
    Ok(settings)
}

pub fn models_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("models")
}

pub fn engine_config(app: &AppHandle, settings: &Settings) -> EngineConfig {
    let vad_path = app
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("resources/models")
        .join(models::SILERO_VAD.file);
    EngineConfig {
        models_dir: models_dir(app),
        vad_path,
        model_id: settings.active_model.clone(),
        use_gpu: settings.use_gpu,
        unload_after_idle_minutes: settings.unload_after_idle_minutes,
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub size_bytes: f64,
    pub multilingual: bool,
    pub downloaded: bool,
    pub downloading: bool,
    pub active: bool,
}

#[tauri::command]
#[specta::specta]
pub fn list_models(app: AppHandle, state: State<'_, AppState>) -> Vec<ModelInfo> {
    let dir = models_dir(&app);
    let active = state
        .settings
        .lock()
        .expect("settings")
        .active_model
        .clone();
    models::CATALOG
        .iter()
        .map(|m| ModelInfo {
            id: m.id.into(),
            name: m.name.into(),
            size_bytes: m.size as f64,
            multilingual: m.multilingual,
            downloaded: m.is_downloaded(&dir),
            downloading: state.downloads.is_active(m.id),
            active: m.id == active,
        })
        .collect()
}

/// Starts a download in the background; progress arrives as `model-download-progress`.
#[tauri::command]
#[specta::specta]
pub fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let def =
        models::find(&id).ok_or_else(|| CommandError::Model(format!("unknown model {id}")))?;
    let Some(ticket) = state.downloads.begin(def.id) else {
        return Ok(()); // already downloading
    };
    let dir = models_dir(&app);
    tauri::async_runtime::spawn(async move {
        let report = |bytes: u64, status: DownloadStatus| {
            let _ = ModelDownloadProgress {
                id: def.id.into(),
                bytes: bytes as f64,
                total: def.size as f64,
                status,
            }
            .emit(&app);
        };
        let mut last = Instant::now();
        let cancel = ticket.cancel_flag();
        let result = match models::download_client() {
            Ok(client) => {
                models::download(&client, def, &def.url(), &dir, cancel, |bytes, _| {
                    if last.elapsed() >= Duration::from_millis(200) {
                        last = Instant::now();
                        report(bytes, DownloadStatus::Downloading);
                    }
                })
                .await
            }
            Err(e) => Err(e),
        };
        // Unregister before reporting, so a `list_models` after the event sees it finished.
        drop(ticket);
        let state = app.state::<AppState>();
        match result {
            Ok(_) => {
                log::info!("downloaded model {}", def.id);
                report(def.size, DownloadStatus::Done);
                let engine = Arc::clone(&state.engine);
                if state.settings.lock().expect("settings").active_model == def.id {
                    engine.preload().await;
                }
            }
            Err(models::DownloadError::Cancelled) => report(0, DownloadStatus::Cancelled),
            Err(e) => {
                log::warn!("model download {} failed: {e}", def.id);
                report(0, DownloadStatus::Failed);
            }
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn cancel_download(state: State<'_, AppState>, id: String) {
    state.downloads.cancel(&id);
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let def =
        models::find(&id).ok_or_else(|| CommandError::Model(format!("unknown model {id}")))?;
    let active = state.settings.lock().expect("settings").active_model == def.id;
    if active {
        state.engine.unload().await;
    }
    def.remove_from(&models_dir(&app))
        .map_err(|e| CommandError::Model(e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn set_active_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Settings, CommandError> {
    let mut settings = state.settings.lock().expect("settings").clone();
    settings.active_model = id;
    apply_settings(&app, &state, settings)
}

#[tauri::command]
#[specta::specta]
pub fn get_engine_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

/// The bubble asks to be shown or hidden (it owns its animations and message timing).
/// `idle` requests honor "Hide for 1 hour".
#[tauri::command]
#[specta::specta]
pub fn overlay_presence(
    app: AppHandle,
    overlay: State<'_, Arc<OverlayState>>,
    visible: bool,
    idle: bool,
) {
    if visible && !(idle && overlay.idle_hidden()) {
        overlay::show_bubble(&app);
    } else {
        overlay::hide_bubble(&app);
    }
}

/// The pill's box inside the bubble window (logical px), for click-through.
#[tauri::command]
#[specta::specta]
pub fn set_pill_bounds(overlay: State<'_, Arc<OverlayState>>, bounds: PillBounds) {
    let valid = [bounds.x, bounds.y, bounds.width, bounds.height]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.0)
        && bounds.width <= 1000.0
        && bounds.height <= 1000.0;
    if valid {
        *overlay.pill.lock().expect("pill bounds") = bounds;
    }
}

#[tauri::command]
#[specta::specta]
pub fn overlay_menu(app: AppHandle) {
    if let Err(e) = overlay::popup_menu(&app) {
        log::warn!("bubble menu failed: {e}");
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_hub(app: AppHandle) {
    overlay::open_hub(&app);
}

fn history_err(e: impl std::fmt::Display) -> CommandError {
    CommandError::History(e.to_string())
}

/// Newest first; `before` is the id to page from. `query` searches text and raw.
#[tauri::command]
#[specta::specta]
pub fn get_history(
    state: State<'_, AppState>,
    before: Option<f64>,
    limit: u32,
    query: String,
) -> Result<Vec<HistoryItem>, CommandError> {
    if query.len() > 200 {
        return Err(CommandError::History("query too long".into()));
    }
    state
        .history
        .page(before.map(|b| b as i64), limit.clamp(1, 200), query.trim())
        .map_err(history_err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_history_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: f64,
) -> Result<(), CommandError> {
    state.history.delete(id as i64).map_err(history_err)?;
    let _ = HistoryChanged.emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn clear_history(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    state.history.clear().map_err(history_err)?;
    let _ = HistoryChanged.emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn copy_history_item(state: State<'_, AppState>, id: f64) -> Result<bool, CommandError> {
    let item = state
        .history
        .get(id as i64)
        .map_err(history_err)?
        .ok_or_else(|| history_err("not found"))?;
    Ok(insert::copy_to_clipboard(&item.text))
}

/// Runs the AI cleanup again on a history item's raw text.
#[tauri::command]
#[specta::specta]
pub async fn retry_cleanup(
    app: AppHandle,
    state: State<'_, AppState>,
    id: f64,
) -> Result<HistoryItem, CommandError> {
    let item = state
        .history
        .get(id as i64)
        .map_err(history_err)?
        .ok_or_else(|| history_err("not found"))?;
    let settings = state.settings.lock().expect("settings").clone();
    let cleanup = configured_cleanup(&settings)?;
    let cleaned = cleanup
        .polish(&item.raw, cleanup.wants_spanish(None), Default::default())
        .await
        .map_err(|e| CommandError::Model(format!("the cleanup failed ({e}); kept the original")))?
        .ok_or_else(|| CommandError::Model("the cleanup was rejected; kept the original".into()))?;
    let text = crate::core::text::apply_replacements(&cleaned, &settings.replacements);
    state
        .history
        .update_text(item.id as i64, &text)
        .map_err(history_err)?;
    let _ = HistoryChanged.emit(&app);
    state
        .history
        .get(item.id as i64)
        .map_err(history_err)?
        .ok_or_else(|| history_err("not found"))
}

#[tauri::command]
#[specta::specta]
pub fn get_stats(state: State<'_, AppState>) -> Result<Stats, CommandError> {
    state.history.stats(history::now_ms()).map_err(history_err)
}

/// Starts "Record new shortcut"; the result arrives as `hotkey-recorded`.
#[tauri::command]
#[specta::specta]
pub fn record_hotkey(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let hotkeys = state
        .hotkeys
        .as_ref()
        .ok_or_else(|| CommandError::Hotkey("global hotkeys unavailable".into()))?;
    hotkeys.start_recording(Box::new(move |hotkey| {
        let conflict = hotkey
            .as_deref()
            .and_then(hotkey_spec::known_conflict)
            .map(str::to_string);
        let _ = HotkeyRecorded { hotkey, conflict }.emit(&app);
    }));
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn cancel_record_hotkey(state: State<'_, AppState>) {
    if let Some(hotkeys) = &state.hotkeys {
        hotkeys.cancel_recording();
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, CommandError> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| CommandError::Storage(e.to_string()))?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
#[specta::specta]
pub fn open_logs_folder(app: AppHandle) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| CommandError::Storage(e.to_string()))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| CommandError::Storage(e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn get_permissions() -> Permissions {
    permissions::status()
}

/// Asks the OS for `kind` the most direct way it has: its own prompt while it still has one,
/// otherwise the exact privacy page where the switch lives.
#[tauri::command]
#[specta::specta]
pub fn request_permission(
    app: AppHandle,
    kind: PermissionKind,
) -> Result<PermissionRequest, CommandError> {
    let outcome = permissions::request(kind);
    if outcome == PermissionRequest::NeedsSettings {
        open_permission_settings(app, kind)?;
    }
    Ok(outcome)
}

/// "Fix permission": clears stale macOS approvals for Burbuja, then asks again the same way
/// `request_permission` does.
#[tauri::command]
#[specta::specta]
pub fn reset_permission(
    app: AppHandle,
    kind: PermissionKind,
) -> Result<PermissionRequest, CommandError> {
    let outcome = permissions::reset(kind, &app.config().identifier);
    if outcome == PermissionRequest::NeedsSettings {
        open_permission_settings(app, kind)?;
    }
    Ok(outcome)
}

/// Relaunches Burbuja. macOS only applies Input Monitoring to a fresh process.
#[tauri::command]
#[specta::specta]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// Opens the OS privacy page for `kind`. On Windows only the microphone applies.
#[tauri::command]
#[specta::specta]
pub fn open_permission_settings(app: AppHandle, kind: PermissionKind) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    let url = match (cfg!(target_os = "macos"), kind) {
        (false, PermissionKind::Microphone) => "ms-settings:privacy-microphone",
        (false, _) => return Ok(()),
        (true, PermissionKind::Microphone) => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        (true, PermissionKind::Accessibility) => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        (true, PermissionKind::InputMonitoring) => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CommandError::Storage(e.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn reset_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, CommandError> {
    let fresh = Settings {
        onboarding_completed: true,
        ..Settings::default()
    };
    apply_settings(&app, &state, fresh)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTest {
    pub provider: String,
    pub before: String,
    /// `None` when the provider failed or the output was rejected by the guards.
    pub after: Option<String>,
    /// Why the provider failed, if it did.
    pub error: Option<crate::events::ErrorCode>,
    pub ms: u32,
}

fn configured_cleanup(settings: &Settings) -> Result<Cleanup, CommandError> {
    Cleanup::from_settings(settings, &cleanup::local_client())
        .ok_or_else(|| CommandError::Model("AI cleanup is off".into()))
}

const TEST_PHRASE: &str = "um so I was thinking uh we should meet on tuesday no wednesday at three";

/// Runs the cleanup on a test phrase with the current settings.
#[tauri::command]
#[specta::specta]
pub async fn test_provider(state: State<'_, AppState>) -> Result<ProviderTest, CommandError> {
    let settings = state.settings.lock().expect("settings").clone();
    let cleanup = configured_cleanup(&settings)?;
    let started = Instant::now();
    // The test phrase is English: no Spanish line, whatever the configured language.
    let after = cleanup.polish(TEST_PHRASE, false, Default::default()).await;
    Ok(ProviderTest {
        provider: cleanup.label(),
        before: TEST_PHRASE.into(),
        error: after.as_ref().err().map(|e| e.code()),
        after: after.ok().flatten(),
        ms: started.elapsed().as_millis() as u32,
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlms {
    pub ollama: Option<Vec<String>>,
    pub lmstudio: Option<Vec<String>>,
}

/// Probes Ollama (:11434) and LM Studio (:1234) with `GET /v1/models`.
#[tauri::command]
#[specta::specta]
pub async fn detect_local_llms() -> LocalLlms {
    let client = cleanup::local_client();
    let (ollama, lmstudio) = tokio::join!(
        cleanup::local_models(&client, cleanup::OLLAMA_URL),
        cleanup::local_models(&client, cleanup::LMSTUDIO_URL),
    );
    LocalLlms { ollama, lmstudio }
}

#[tauri::command]
#[specta::specta]
pub fn list_microphones() -> Result<Vec<MicInfo>, CommandError> {
    audio::list_microphones().map_err(|e| CommandError::Microphone(e.to_string()))
}

/// Opens `device` (`""` = default) for the settings meter. Levels arrive as `mic-level` on the
/// hub window only; no audio is kept.
#[tauri::command]
#[specta::specta]
pub fn start_mic_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    device: String,
) -> Result<(), CommandError> {
    if device.len() > 512 {
        return Err(CommandError::Microphone("device name too long".into()));
    }
    let mut preview = state.preview.lock().expect("preview");
    drop(preview.take());
    let on_level = Box::new(move |levels: [f32; 16], peak: f32| {
        let _ = MicLevel {
            levels: levels.to_vec(),
            peak,
        }
        .emit_to(&app, "hub");
    });
    let (mic, _) = Mic::start(&device, false, on_level, |e| {
        log::warn!("mic preview failed: {e}")
    })
    .map_err(|e| CommandError::Microphone(e.to_string()))?;
    *preview = Some(mic);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn stop_mic_preview(state: State<'_, AppState>) {
    drop(state.preview.lock().expect("preview").take());
}

#[tauri::command]
#[specta::specta]
pub fn play_sound(state: State<'_, AppState>, kind: SoundKind) {
    state.sounds.play(kind);
}

#[tauri::command]
#[specta::specta]
pub fn start_hands_free(state: State<'_, AppState>) {
    state.controller.send(Msg::StartHandsFree);
}

#[tauri::command]
#[specta::specta]
pub fn cancel_dictation(state: State<'_, AppState>) {
    state.controller.send(Msg::Cancel);
}

#[tauri::command]
#[specta::specta]
pub fn get_last_timings() -> Option<crate::events::DictationTimings> {
    crate::dictation::controller::last_timings()
}
