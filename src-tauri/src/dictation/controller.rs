//! The dictation controller: the **only** owner of dictation state.
//!
//! It runs on its own thread with an mpsc inbox. Hotkey events, timers, UI intents and job
//! results all arrive as [`Msg`]; it feeds them to [`Gestures`] and performs the resulting
//! [`Action`]. Timers run on the async runtime and report back through the same inbox. Every
//! job carries an id, and results from stale jobs are dropped.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::audio::{Mic, MicError, Recording};
use crate::core::gestures::{Action, DictationState as GState, Gestures, Input};
use crate::core::guards::{RecordingVerdict, check_recording};
use crate::core::hotkey_spec::HotkeyEvent;
use crate::engine::EngineHost;
use crate::events::HistoryChanged;
use crate::history::{History, NewDictation, now_ms};
use crate::insert::Inserted;
use tokio_util::sync::CancellationToken;

use super::pipeline::{self, PipelineOutcome};
use crate::events::{
    DictationError, DictationResult, DictationState, DictationTimings, ErrorCode, IdleReason,
    MicLevel, PermissionsNeeded, RecordMode, UiState,
};
use crate::hotkeys::BindingId;
use crate::permissions::{self, PermissionKind, PermissionState};
use crate::settings::Settings;
use crate::sound::{SoundKind, SoundPlayer};

/// What a binding does. Only dictation in v2.0; `AskScreen` is future work (an extension point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Dictate,
}

fn mode_for(_binding: BindingId) -> Mode {
    Mode::Dictate
}

/// The last finished dictation's timings, for Advanced → "Last dictation timings".
static LAST_TIMINGS: Mutex<Option<DictationTimings>> = Mutex::new(None);

pub fn last_timings() -> Option<DictationTimings> {
    LAST_TIMINGS.lock().ok().and_then(|t| t.clone())
}

pub enum Msg {
    Hotkey {
        binding: BindingId,
        event: HotkeyEvent,
        at: Instant,
    },
    TapTimedOut(u32),
    MaxDuration(u64),
    StartHandsFree,
    Cancel,
    MicFailed(u64, String),
    /// The job's LLM cleanup started ("Polishing").
    Polishing(u64),
    JobDone {
        job: u64,
        outcome: PipelineOutcome,
        latency: Duration,
        audio: Duration,
    },
    Settings(Box<Settings>),
}

#[derive(Clone)]
pub struct ControllerHandle {
    tx: UnboundedSender<Msg>,
}

impl ControllerHandle {
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }
}

struct Controller {
    app: AppHandle,
    tx: UnboundedSender<Msg>,
    sounds: SoundPlayer,
    settings: Settings,
    gestures: Gestures,
    mic: Option<Mic>,
    /// Bumped per recording; stale max-duration timers and mic failures are ignored.
    session: u64,
    /// Bumped per transcription job; stale results are dropped.
    job: u64,
    job_task: Option<tauri::async_runtime::JoinHandle<()>>,
    job_cancel: Option<CancellationToken>,
    polishing: bool,
    started: Option<(Instant, f64)>,
    /// hotkey→record and release→stop of the current dictation, in ms.
    record_ms: f64,
    stop_ms: f64,
    results: u32,
    engine: Arc<EngineHost>,
    client: reqwest::Client,
    history: Arc<History>,
}

pub fn spawn(
    app: AppHandle,
    sounds: SoundPlayer,
    settings: Settings,
    engine: Arc<EngineHost>,
    history: Arc<History>,
) -> ControllerHandle {
    let (tx, rx) = unbounded_channel();
    let controller = Controller {
        app,
        tx: tx.clone(),
        sounds,
        settings,
        engine,
        history,
        gestures: Gestures::new(),
        mic: None,
        session: 0,
        job: 0,
        job_task: None,
        job_cancel: None,
        polishing: false,
        started: None,
        record_ms: 0.0,
        stop_ms: 0.0,
        results: 0,
        client: crate::cleanup::local_client(),
    };
    thread::Builder::new()
        .name("burbuja-controller".into())
        .spawn(move || controller.run(rx))
        .expect("spawn controller thread");
    ControllerHandle { tx }
}

impl Controller {
    fn run(mut self, mut rx: UnboundedReceiver<Msg>) {
        while let Some(msg) = rx.blocking_recv() {
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Hotkey { binding, event, at } => {
                let Mode::Dictate = mode_for(binding);
                let input = match event {
                    HotkeyEvent::Pressed => Input::Pressed(at),
                    HotkeyEvent::Released => Input::Released(at),
                    HotkeyEvent::Chord => Input::Chord,
                    HotkeyEvent::Escape => Input::Escape,
                };
                self.step(input, Some(at));
            }
            Msg::TapTimedOut(token) => self.step(Input::TapTimedOut(token), None),
            Msg::MaxDuration(session) if session == self.session => {
                self.step(Input::MaxDurationReached, None)
            }
            Msg::MaxDuration(_) => {}
            // A click on the bubble: start hands-free when idle, finish when recording.
            // In hands-free a press finishes; while holding it is ignored.
            Msg::StartHandsFree if self.gestures.is_recording() => {
                self.step(Input::Pressed(Instant::now()), None)
            }
            Msg::StartHandsFree => self.step(Input::StartHandsFree, Some(Instant::now())),
            Msg::Cancel => self.step(Input::Escape, None),
            Msg::MicFailed(session, error)
                if session == self.session && self.gestures.is_recording() =>
            {
                log::warn!("microphone failed while recording: {error}");
                self.mic = None;
                self.gestures.step(Input::Reset);
                self.error(ErrorCode::MicUnavailable, "Microphone unavailable");
                self.emit_state(None);
            }
            Msg::MicFailed(..) => {}
            Msg::JobDone {
                job,
                outcome,
                latency,
                audio,
            } => self.job_done(job, outcome, latency, audio),
            Msg::Polishing(job)
                if job == self.job && self.gestures.state() == GState::Processing =>
            {
                self.polishing = true;
                self.emit_state(None);
            }
            Msg::Polishing(_) => {}
            Msg::Settings(settings) => self.settings = *settings,
        }
    }

    fn step(&mut self, input: Input, at: Option<Instant>) {
        let before = self.gestures.state();
        let action = self.gestures.step(input);
        log::debug!(
            "gesture {before:?} + {input:?} -> {:?} / {action:?}",
            self.gestures.state()
        );
        match action {
            Action::None => {}
            Action::StartRecording => self.start_recording(at.unwrap_or_else(Instant::now)),
            Action::AwaitSecondTap => {
                let token = self.gestures.tap_token();
                let window = Duration::from_millis(self.settings.double_tap_window_ms.into());
                self.after(window, Msg::TapTimedOut(token));
            }
            Action::EnterHandsFree => self.emit_state(None),
            Action::DiscardRecording => {
                drop(self.mic.take());
                self.emit_state(None);
            }
            Action::CancelRecording => {
                drop(self.mic.take());
                self.play(SoundKind::Cancel);
                self.emit_state(Some(IdleReason::Cancelled));
            }
            Action::FinishRecording => self.finish_recording(),
            Action::CancelProcessing => {
                // Stop the HTTP requests and ask the worker to stop (killed after 500 ms if it
                // doesn't), then forget the job.
                if let Some(cancel) = self.job_cancel.take() {
                    cancel.cancel();
                }
                self.engine.cancel(self.job);
                self.job += 1;
                if let Some(task) = self.job_task.take() {
                    task.abort();
                }
                self.play(SoundKind::Cancel);
                self.emit_state(Some(IdleReason::Cancelled));
            }
        }
    }

    /// Checked before opening the device: a blocked microphone still opens on macOS and Windows
    /// but records pure silence, which looked like "the app doesn't work".
    fn microphone_allowed(&self) -> bool {
        match permissions::microphone_state() {
            PermissionState::Granted => true,
            PermissionState::NotAsked => {
                // First dictation before onboarding asked: macOS shows its own prompt now.
                permissions::request(PermissionKind::Microphone);
                self.error(ErrorCode::MicPermission, "Allow microphone access");
                false
            }
            PermissionState::Denied => {
                log::warn!("microphone blocked by the OS privacy settings");
                self.error(ErrorCode::MicPermission, "Microphone access is off");
                self.show_permissions(PermissionKind::Microphone);
                false
            }
        }
    }

    /// Opens the Hub on the permissions page so the fix is one click away.
    fn show_permissions(&self, kind: PermissionKind) {
        crate::overlay::open_hub(&self.app);
        let _ = PermissionsNeeded { kind }.emit(&self.app);
    }

    fn start_recording(&mut self, pressed_at: Instant) {
        if !self.microphone_allowed() {
            self.gestures.step(Input::Reset);
            self.emit_state(None);
            return;
        }
        // Reload an unloaded model while the user speaks. No-op when loaded.
        let engine = Arc::clone(&self.engine);
        tauri::async_runtime::spawn(async move { engine.preload().await });
        self.session += 1;
        let session = self.session;
        let app = self.app.clone();
        let on_level = Box::new(move |levels: [f32; 16], peak: f32| {
            // Only the bubble draws the waveform while recording.
            let _ = MicLevel {
                levels: levels.to_vec(),
                peak,
            }
            .emit_to(&app, crate::overlay::BUBBLE);
        });
        let tx = self.tx.clone();
        let on_failure = move |e: String| {
            let _ = tx.send(Msg::MicFailed(session, e));
        };
        // Capture first, then the start sound.
        match Mic::start(&self.settings.microphone, true, on_level, on_failure) {
            Ok((mic, used_fallback)) => {
                let record_ms = pressed_at.elapsed().as_secs_f64() * 1000.0;
                if used_fallback {
                    log::warn!(
                        "microphone {:?} not found; using the default",
                        self.settings.microphone
                    );
                }
                self.mic = Some(mic);
                self.play(SoundKind::Start);
                let sound_ms = pressed_at.elapsed().as_secs_f64() * 1000.0;
                log::info!(
                    "timing hotkey->record={record_ms:.1}ms hotkey->start_sound_queued={sound_ms:.1}ms"
                );
                self.record_ms = record_ms;
                self.started = Some((Instant::now(), now_ms() as f64));
                let limit = Duration::from_secs(u64::from(self.settings.max_session_minutes) * 60);
                self.after(limit, Msg::MaxDuration(session));
                self.emit_state(None);
            }
            Err(e) => {
                log::warn!("could not open the microphone: {e}");
                self.gestures.step(Input::Reset);
                match e {
                    MicError::PermissionDenied => {
                        self.error(ErrorCode::MicPermission, "Microphone access is off");
                        self.show_permissions(PermissionKind::Microphone);
                    }
                    MicError::NoDevice => {
                        self.error(ErrorCode::NoMicrophone, "No microphone connected");
                    }
                    MicError::Unavailable(_) => {
                        self.error(ErrorCode::MicUnavailable, "Microphone unavailable")
                    }
                }
                self.emit_state(None);
            }
        }
    }

    fn finish_recording(&mut self) {
        let released = Instant::now();
        let recording = self.mic.take().map(Mic::stop).unwrap_or_default();
        self.play(SoundKind::Stop);
        self.stop_ms = released.elapsed().as_secs_f64() * 1000.0;
        log::info!("timing release->stop_sound_queued={:.1}ms", self.stop_ms);
        match check_recording(recording.duration, recording.peak) {
            RecordingVerdict::TooShort => self.give_up(ErrorCode::TooShort, "Too short".into()),
            RecordingVerdict::Silent => {
                log::warn!("digital silence from {:?}", recording.device);
                let message = format!("No sound from {}. Pick another mic.", recording.device);
                self.give_up(ErrorCode::MicSilent, message);
            }
            RecordingVerdict::Transcribe => {
                if self.settings.keep_audio {
                    self.keep_audio(&recording.samples);
                }
                self.transcribe(recording, released)
            }
        }
    }

    /// "Keep audio of dictations" (off by default): a 16 kHz WAV in
    /// `{app_data}/audio/`, written off the controller thread. Never uploaded.
    fn keep_audio(&self, samples: &[f32]) {
        let Some(dir) = self.history.audio_dir().map(Path::to_path_buf) else {
            return;
        };
        let wav = crate::sound::synth::to_wav(samples, crate::audio::capture::SAMPLE_RATE);
        let name = format!("{}.wav", now_ms());
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) =
                std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(name), wav))
            {
                log::warn!("could not keep the audio: {e}");
            }
        });
    }

    /// Runs the post-recording pipeline (transcribe → cleanup → replacements → insert) as one
    /// cancelable task.
    fn transcribe(&mut self, recording: Recording, released: Instant) {
        self.job += 1;
        self.polishing = false;
        let job = self.job;
        let tx = self.tx.clone();
        let audio = recording.duration;
        let cancel = CancellationToken::new();
        self.job_cancel = Some(cancel.clone());
        let polishing_tx = self.tx.clone();
        let input = pipeline::Input {
            job,
            samples: recording.samples,
            settings: self.settings.clone(),
            engine: Arc::clone(&self.engine),
            client: self.client.clone(),
            cancel,
            on_polishing: Box::new(move || {
                let _ = polishing_tx.send(Msg::Polishing(job));
            }),
        };
        self.job_task = Some(tauri::async_runtime::spawn(async move {
            let outcome = pipeline::run(input).await;
            let _ = tx.send(Msg::JobDone {
                job,
                outcome,
                latency: released.elapsed(),
                audio,
            });
        }));
        self.emit_state(None);
    }

    fn job_done(&mut self, job: u64, outcome: PipelineOutcome, latency: Duration, audio: Duration) {
        if job != self.job || self.gestures.state() != GState::Processing {
            return; // cancelled or superseded: never touches the new state
        }
        self.job_task = None;
        self.job_cancel = None;
        self.polishing = false;
        match outcome {
            PipelineOutcome::Done {
                text,
                stages,
                cleanup_failed,
                target_app,
                raw,
                engine,
                inserted,
            } => {
                let timings = DictationTimings {
                    hotkey_to_record_ms: self.record_ms,
                    release_to_stop_ms: self.stop_ms,
                    audio_ms: audio.as_secs_f64() * 1000.0,
                    vad_ms: stages.vad_ms,
                    transcribe_ms: stages.transcribe_ms,
                    cleanup_ms: stages.cleanup_ms,
                    insert_ms: stages.insert_ms,
                    total_ms: latency.as_secs_f64() * 1000.0,
                    engine: engine.clone(),
                };
                log::info!(
                    "timing hotkey->record={:.0}ms release->stop={:.0}ms vad={:.0}ms transcribe={:.0}ms cleanup={:?} insert={:.0}ms total={:.0}ms",
                    timings.hotkey_to_record_ms,
                    timings.release_to_stop_ms,
                    timings.vad_ms,
                    timings.transcribe_ms,
                    timings.cleanup_ms.map(f64::round),
                    timings.insert_ms,
                    timings.total_ms
                );
                if let Ok(mut last) = LAST_TIMINGS.lock() {
                    *last = Some(timings.clone());
                }
                let _ = timings.emit(&self.app);
                let id = self
                    .history
                    .insert(&NewDictation {
                        created_at: now_ms(),
                        text: &text,
                        raw: &raw,
                        engine: &engine,
                        duration_ms: audio.as_millis() as i64,
                        latency_ms: latency.as_millis() as i64,
                        target_app: target_app.as_deref(),
                    })
                    .inspect_err(|e| log::warn!("could not save to history: {e}"))
                    .unwrap_or(0);
                let _ = HistoryChanged.emit(&self.app);
                self.results = id as u32;
                // Never log the transcript itself: only its size and timings.
                log::info!(
                    "dictation done chars={} audio={}ms release->inserted={}ms via {inserted:?}",
                    text.chars().count(),
                    audio.as_millis(),
                    latency.as_millis()
                );
                if inserted == Inserted::ElevatedTarget {
                    self.error(ErrorCode::ElevatedTarget, "Admin window: press Ctrl+V");
                } else if inserted == Inserted::InputBlocked
                    && !permissions::accessibility_granted()
                {
                    self.error(
                        ErrorCode::AccessibilityMissing,
                        "Copied. Allow Accessibility to paste",
                    );
                    self.show_permissions(PermissionKind::Accessibility);
                } else if inserted == Inserted::InputBlocked {
                    self.error(ErrorCode::InputBlocked, "Couldn't paste: press Cmd+V");
                } else if let Some(code) = cleanup_failed {
                    log::info!("inserted the raw text; cleanup failed with {code:?}");
                    self.error(ErrorCode::CleanupSkipped, "Inserted without AI cleanup");
                }
                let _ = DictationResult {
                    id: self.results,
                    raw,
                    text,
                    duration_ms: audio.as_millis() as u32,
                    engine,
                    latency_ms: latency.as_millis() as u32,
                }
                .emit(&self.app);
            }
            PipelineOutcome::Failed(code, message) => self.error(code, &message),
            PipelineOutcome::Aborted => {}
        }
        self.gestures.step(Input::ProcessingFinished);
        self.emit_state(None);
    }

    fn give_up(&mut self, code: ErrorCode, message: String) {
        self.error(code, &message);
        self.gestures.step(Input::ProcessingFinished);
        self.emit_state(None);
    }

    fn after(&self, delay: Duration, msg: Msg) {
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(msg);
        });
    }

    fn play(&self, kind: SoundKind) {
        if self.settings.sounds_enabled {
            self.sounds.play(kind);
        }
    }

    fn error(&self, code: ErrorCode, message: &str) {
        let _ = DictationError {
            code,
            message: message.into(),
        }
        .emit(&self.app);
    }

    fn emit_state(&mut self, reason: Option<IdleReason>) {
        let state = match self.gestures.state() {
            GState::Idle => UiState::Idle,
            GState::Holding | GState::AwaitingSecondTap => UiState::Listening,
            GState::HandsFree => UiState::HandsFree,
            GState::Processing if self.polishing => UiState::Polishing,
            GState::Processing => UiState::Processing,
        };
        let mode = match self.gestures.state() {
            GState::Holding | GState::AwaitingSecondTap => Some(RecordMode::Hold),
            GState::HandsFree => Some(RecordMode::HandsFree),
            _ => None,
        };
        if state == UiState::Idle {
            self.started = None;
        }
        let limit = u64::from(self.settings.max_session_minutes) * 60_000;
        let payload = DictationState {
            state,
            mode,
            started_at: self.started.map(|(_, epoch)| epoch),
            elapsed_ms: self
                .started
                .map_or(0, |(t, _)| t.elapsed().as_millis() as u32),
            limit_ms: limit as u32,
            reason,
        };
        let _ = payload.emit(&self.app);
    }
}
