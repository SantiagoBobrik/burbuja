//! Engine host: owns the worker subprocess.
//!
//! - One request at a time (an async mutex around the worker connection).
//! - `preload()` spawns, loads and warms up in the background; `transcribe()` does it lazily too.
//! - `cancel(job)` sends `Cancel`; if the worker hasn't answered within 500 ms it is killed and
//!   respawned on next use.
//! - A crash fails the in-flight job with `Crashed`; the next request respawns with backoff.
//! - If the GPU path fails to load or crashes while loading, the worker is restarted CPU-only
//!   and the status says so (once).
//! - The model is unloaded after `unload_after_idle_minutes` without use.

use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::models;
use super::protocol::{LoadedInfo, Request, Response, TranscriptMsg, recv_frame, send_frame};
use super::worker::{CPU_ONLY_FLAG, WORKER_FLAG};

const CANCEL_GRACE: Duration = Duration::from_millis(500);
const MAX_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EngineState {
    Unloaded,
    Loading,
    Ready,
    ModelMissing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EngineState,
    pub model: String,
    pub on_gpu: bool,
    pub device: String,
    /// Set when the GPU failed and the engine fell back to the CPU.
    pub gpu_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub models_dir: PathBuf,
    pub vad_path: PathBuf,
    pub model_id: String,
    pub use_gpu: bool,
    pub unload_after_idle_minutes: u32,
}

#[derive(Debug)]
pub enum Outcome {
    Text(TranscriptMsg),
    NoSpeech,
    Aborted,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("model not downloaded")]
    ModelMissing,
    #[error("the engine stopped unexpectedly")]
    Crashed,
    #[error("{0}")]
    Failed(String),
}

enum Event {
    Msg(Response),
    Exited,
}

struct Conn {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    rx: UnboundedReceiver<Event>,
    loaded: Option<(String, LoadedInfo)>,
    cpu_only: bool,
}

pub type StatusSink = Box<dyn Fn(EngineStatus) + Send + Sync>;

/// The worker's process and stdin, shared so `cancel` can act while a request holds `conn`.
type CancelHandles = (Arc<Mutex<Child>>, Arc<Mutex<ChildStdin>>);

pub struct EngineHost {
    config: Mutex<EngineConfig>,
    conn: tokio::sync::Mutex<Option<Conn>>,
    /// Handles for `cancel`, usable while a request holds `conn`.
    cancel_handles: Mutex<Option<CancelHandles>>,
    inflight: AtomicU64,
    last_used: Mutex<Instant>,
    failures: Mutex<u32>,
    gpu_failed: Mutex<bool>,
    on_status: StatusSink,
    status: Mutex<EngineStatus>,
}

impl EngineHost {
    pub fn new(config: EngineConfig, on_status: StatusSink) -> Arc<Self> {
        let status = EngineStatus {
            state: EngineState::Unloaded,
            model: config.model_id.clone(),
            on_gpu: false,
            device: String::new(),
            gpu_fallback: false,
        };
        let host = Arc::new(Self {
            config: Mutex::new(config),
            conn: tokio::sync::Mutex::new(None),
            cancel_handles: Mutex::new(None),
            inflight: AtomicU64::new(0),
            last_used: Mutex::new(Instant::now()),
            failures: Mutex::new(0),
            gpu_failed: Mutex::new(false),
            on_status,
            status: Mutex::new(status),
        });
        Self::spawn_idle_watch(Arc::downgrade(&host));
        host
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().expect("status").clone()
    }

    /// Applies new settings. A model or GPU change unloads the current worker.
    pub fn configure(self: &Arc<Self>, config: EngineConfig) {
        let changed = {
            let mut current = self.config.lock().expect("config");
            let changed = current.model_id != config.model_id || current.use_gpu != config.use_gpu;
            *current = config;
            changed
        };
        if changed {
            *self.gpu_failed.lock().expect("gpu flag") = false;
            let host = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                host.unload().await;
                host.preload().await;
            });
        }
    }

    /// Spawns, loads and warms up the model if needed. Cheap when already loaded.
    pub async fn preload(self: &Arc<Self>) {
        *self.last_used.lock().expect("last used") = Instant::now();
        let mut conn = self.conn.lock().await;
        if let Err(e) = self.ensure_loaded(&mut conn, true).await {
            log::warn!("engine preload failed: {e}");
        }
    }

    pub async fn unload(&self) {
        let mut conn = self.conn.lock().await;
        if let Some(c) = conn.take() {
            kill(&c.child);
        }
        *self.cancel_handles.lock().expect("handles") = None;
        self.set_state(EngineState::Unloaded, None);
    }

    pub async fn transcribe(
        self: &Arc<Self>,
        job: u64,
        pcm: Vec<f32>,
        language: Option<String>,
        vocabulary: Vec<String>,
    ) -> Result<Outcome, EngineError> {
        *self.last_used.lock().expect("last used") = Instant::now();
        let mut guard = self.conn.lock().await;
        self.ensure_loaded(&mut guard, false).await?;
        let conn = guard.as_mut().expect("loaded");
        let vocabulary = if conn
            .loaded
            .as_ref()
            .is_some_and(|(_, i)| i.supports_vocabulary)
        {
            vocabulary
        } else {
            Vec::new()
        };
        self.inflight.store(job, Ordering::SeqCst);
        let sent = send(
            &conn.stdin,
            &Request::Run {
                job,
                language,
                vocabulary,
            },
            &pcm,
        );
        drop(pcm);
        let result = match sent {
            Ok(()) => wait_for_job(conn, job).await,
            Err(_) => Err(EngineError::Crashed),
        };
        self.inflight.store(0, Ordering::SeqCst);
        *self.last_used.lock().expect("last used") = Instant::now();
        if matches!(result, Err(EngineError::Crashed)) {
            self.drop_conn(&mut guard);
        }
        result
    }

    /// Asks the worker to abort `job`; kills it if it doesn't stop within 500 ms.
    pub fn cancel(self: &Arc<Self>, job: u64) {
        if self.inflight.load(Ordering::SeqCst) != job {
            return;
        }
        let Some((child, stdin)) = self.cancel_handles.lock().expect("handles").clone() else {
            return;
        };
        let _ = send(&stdin, &Request::Cancel { job }, &[]);
        let host = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(CANCEL_GRACE).await;
            if host.inflight.load(Ordering::SeqCst) == job {
                log::warn!("engine did not stop within {CANCEL_GRACE:?}; killing it");
                kill(&child);
            }
        });
    }

    async fn ensure_loaded(
        &self,
        conn: &mut Option<Conn>,
        warm_up: bool,
    ) -> Result<(), EngineError> {
        let config = self.config.lock().expect("config").clone();
        let Some(def) = models::find(&config.model_id) else {
            self.set_state(EngineState::ModelMissing, None);
            return Err(EngineError::ModelMissing);
        };
        if !def.is_downloaded(&config.models_dir) {
            self.set_state(EngineState::ModelMissing, None);
            return Err(EngineError::ModelMissing);
        }
        if conn
            .as_ref()
            .is_some_and(|c| c.loaded.as_ref().is_some_and(|(id, _)| id == def.id))
        {
            return Ok(());
        }

        self.set_state(EngineState::Loading, None);
        let cpu_only = !config.use_gpu || *self.gpu_failed.lock().expect("gpu flag");
        match self
            .spawn_and_load(conn, &config, def, cpu_only, warm_up)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if !cpu_only => {
                log::warn!("GPU engine failed ({e}); falling back to CPU");
                *self.gpu_failed.lock().expect("gpu flag") = true;
                self.drop_conn(conn);
                self.spawn_and_load(conn, &config, def, true, warm_up).await
            }
            Err(e) => Err(e),
        }
        .inspect_err(|e| {
            self.drop_conn(conn);
            self.set_state(EngineState::Failed, None);
            log::error!("engine unavailable: {e}");
        })
    }

    async fn spawn_and_load(
        &self,
        conn: &mut Option<Conn>,
        config: &EngineConfig,
        def: &models::ModelDef,
        cpu_only: bool,
        warm_up: bool,
    ) -> Result<(), EngineError> {
        if conn.as_ref().is_none_or(|c| c.cpu_only != cpu_only) {
            self.drop_conn(conn);
            self.backoff().await;
            *conn = Some(self.spawn(cpu_only)?);
        }
        let c = conn.as_mut().expect("spawned");
        expect(c, |r| matches!(r, Response::Ready { .. })).await?;

        let model = def.path_in(&config.models_dir);
        send(
            &c.stdin,
            &Request::Load {
                model,
                arch: def.arch,
                vad: config.vad_path.clone(),
            },
            &[],
        )
        .map_err(|_| EngineError::Crashed)?;
        let info = match expect(c, |r| matches!(r, Response::Loaded(_))).await? {
            Response::Loaded(info) => info,
            _ => unreachable!(),
        };
        log::info!(
            "engine loaded {} on {} ({}) in {} ms",
            def.id,
            info.device,
            info.backend,
            info.load_ms
        );
        c.loaded = Some((def.id.to_string(), info.clone()));
        if warm_up {
            send(&c.stdin, &Request::WarmUp, &[]).map_err(|_| EngineError::Crashed)?;
            if let Response::WarmedUp { ms } =
                expect(c, |r| matches!(r, Response::WarmedUp { .. })).await?
            {
                log::info!("engine warm-up {ms} ms");
            }
        }
        *self.failures.lock().expect("failures") = 0;
        self.set_state(EngineState::Ready, Some(&info));
        Ok(())
    }

    fn spawn(&self, cpu_only: bool) -> Result<Conn, EngineError> {
        // Our own executable by absolute path: no PATH lookup, no shell.
        let exe = std::env::current_exe().map_err(|e| EngineError::Failed(e.to_string()))?;
        let mut cmd = Command::new(&exe);
        // Run from the install directory, never an inherited working directory: native code
        // that loads libraries by bare name (ggml's default search, a missing vulkan-1.dll)
        // must not find a DLL planted in whatever folder Burbuja was launched from.
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.arg(WORKER_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if cpu_only {
            cmd.arg(CPU_ONLY_FLAG);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::Failed(format!("spawn worker: {e}")))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = unbounded_channel();
        thread::Builder::new()
            .name("engine-host-out".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(frame)) = recv_frame::<Response>(&mut reader) {
                    if tx.send(Event::Msg(frame.message)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Event::Exited);
            })
            .map_err(|e| EngineError::Failed(e.to_string()))?;
        let child = Arc::new(Mutex::new(child));
        let stdin = Arc::new(Mutex::new(stdin));
        *self.cancel_handles.lock().expect("handles") =
            Some((Arc::clone(&child), Arc::clone(&stdin)));
        Ok(Conn {
            child,
            stdin,
            rx,
            loaded: None,
            cpu_only,
        })
    }

    fn drop_conn(&self, conn: &mut Option<Conn>) {
        if let Some(c) = conn.take() {
            kill(&c.child);
            *self.failures.lock().expect("failures") += 1;
        }
        *self.cancel_handles.lock().expect("handles") = None;
    }

    async fn backoff(&self) {
        let failures = *self.failures.lock().expect("failures");
        if failures > 0 {
            let delay =
                (Duration::from_millis(200) * 2u32.saturating_pow(failures - 1)).min(MAX_BACKOFF);
            tokio::time::sleep(delay).await;
        }
    }

    fn set_state(&self, state: EngineState, info: Option<&LoadedInfo>) {
        let status = {
            let mut s = self.status.lock().expect("status");
            s.state = state;
            s.model = self.config.lock().expect("config").model_id.clone();
            s.gpu_fallback = *self.gpu_failed.lock().expect("gpu flag");
            if let Some(info) = info {
                s.on_gpu = info.on_gpu;
                s.device = info.device.clone();
            }
            s.clone()
        };
        (self.on_status)(status);
    }

    fn spawn_idle_watch(host: std::sync::Weak<Self>) {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(host) = host.upgrade() else { return };
                let minutes = host
                    .config
                    .lock()
                    .expect("config")
                    .unload_after_idle_minutes;
                let idle = host.last_used.lock().expect("last used").elapsed();
                if minutes > 0
                    && idle > Duration::from_secs(u64::from(minutes) * 60)
                    && host.inflight.load(Ordering::SeqCst) == 0
                    && let Ok(mut conn) = host.conn.try_lock()
                    && conn.is_some()
                {
                    log::info!("unloading model after {minutes} idle minutes");
                    if let Some(c) = conn.take() {
                        kill(&c.child);
                    }
                    *host.cancel_handles.lock().expect("handles") = None;
                    host.set_state(EngineState::Unloaded, None);
                }
            }
        });
    }
}

async fn wait_for_job(conn: &mut Conn, job: u64) -> Result<Outcome, EngineError> {
    loop {
        match conn.rx.recv().await {
            Some(Event::Msg(Response::Transcript(t))) if t.job == job => {
                return Ok(Outcome::Text(t));
            }
            Some(Event::Msg(Response::NoSpeech { job: j })) if j == job => {
                return Ok(Outcome::NoSpeech);
            }
            Some(Event::Msg(Response::Aborted { job: j })) if j == job => {
                return Ok(Outcome::Aborted);
            }
            Some(Event::Msg(Response::Error {
                job: Some(j),
                message,
            })) if j == job => {
                return Err(EngineError::Failed(message));
            }
            Some(Event::Msg(_)) => {} // stale or unrelated
            Some(Event::Exited) | None => return Err(EngineError::Crashed),
        }
    }
}

/// Waits for the first response matching `want`; a job-less `Error` or an exit fails it.
async fn expect(
    conn: &mut Conn,
    want: impl Fn(&Response) -> bool,
) -> Result<Response, EngineError> {
    loop {
        match conn.rx.recv().await {
            Some(Event::Msg(r)) if want(&r) => return Ok(r),
            Some(Event::Msg(Response::Error { job: None, message })) => {
                return Err(EngineError::Failed(message));
            }
            Some(Event::Msg(_)) => {}
            Some(Event::Exited) | None => return Err(EngineError::Crashed),
        }
    }
}

fn send(stdin: &Arc<Mutex<ChildStdin>>, request: &Request, pcm: &[f32]) -> std::io::Result<()> {
    let mut stdin = stdin.lock().expect("stdin");
    send_frame(&mut *stdin, request, pcm)
}

fn kill(child: &Arc<Mutex<Child>>) {
    let mut child = child.lock().expect("child");
    let _ = child.kill();
    let _ = child.wait();
}
