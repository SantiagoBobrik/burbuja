//! The engine worker process (`burbuja --engine-worker`): the only process that runs
//! whisper.cpp, so a crash in a GPU driver or a model ends it and never the app.
//!
//! Requests arrive on stdin and are answered in order on stdout. A reader thread decodes stdin:
//! `Cancel` takes effect immediately (it raises the running job's abort flag), everything else
//! goes to the main thread's queue. The worker exits as soon as the app closes stdin.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Instant;

use super::native::{self, Arch, DeviceKind, Model, RunError, RunOptions, SAMPLE_RATE, Vad};
use super::protocol::{
    Frame, LoadedInfo, Request, Response, TranscriptMsg, recv_frame, send_frame,
};
use crate::core::vad_trim::speech_range;

pub const WORKER_FLAG: &str = "--engine-worker";
pub const CPU_ONLY_FLAG: &str = "--cpu-only";

/// VAD: a frame counts as speech above this probability; shorter bursts are ignored.
pub const VAD_THRESHOLD: f32 = 0.5;
pub const VAD_MIN_SPEECH_MS: u32 = 250;

pub fn is_worker_process() -> bool {
    std::env::args_os().any(|a| a == WORKER_FLAG)
}

/// The job being transcribed (0 when idle) and its abort flag, shared with the reader thread.
#[derive(Default)]
struct Current {
    job: AtomicU64,
    abort: AtomicBool,
}

struct Loaded {
    model: Model,
    vad: Vad,
}

struct Worker {
    out: File,
    use_gpu: bool,
    loaded: Option<Loaded>,
    current: Arc<Current>,
}

/// Runs the worker until the app closes stdin. Returns the process exit code.
pub fn run() -> i32 {
    let out = match native::reserve_stdout() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("engine worker: {e}");
            return 2;
        }
    };
    let current = Arc::new(Current::default());
    let (queue, requests) = mpsc::channel();
    let reader_current = Arc::clone(&current);
    if let Err(e) = thread::Builder::new()
        .name("engine-worker-stdin".into())
        .spawn(move || read_stdin(&queue, &reader_current))
    {
        eprintln!("engine worker: {e}");
        return 2;
    }

    let cpu_only = std::env::args_os().any(|a| a == CPU_ONLY_FLAG);
    native::init(cpu_only);
    let mut worker = Worker {
        out,
        use_gpu: !cpu_only,
        loaded: None,
        current,
    };
    let devices = native::devices()
        .into_iter()
        .map(|d| format!("{} ({}, {:?})", d.description, d.backend, d.kind))
        .collect();
    if !worker.reply(&Response::Ready { devices }) {
        return 1;
    }
    for Frame { message, audio } in requests {
        let response = worker.handle(message, &audio);
        if !worker.reply(&response) {
            return 1;
        }
    }
    0
}

impl Worker {
    fn reply(&mut self, response: &Response) -> bool {
        send_frame(&mut self.out, response, &[]).is_ok()
    }

    fn handle(&mut self, request: Request, audio: &[f32]) -> Response {
        match request {
            Request::Load { model, arch, vad } => {
                self.loaded = None; // never two models in memory
                match self.load(&model, arch.into(), &vad) {
                    Ok(info) => Response::Loaded(info),
                    Err(message) => Response::Error { job: None, message },
                }
            }
            Request::WarmUp => self.warm_up(),
            Request::Run {
                job,
                language,
                vocabulary,
            } => {
                self.current.abort.store(false, Ordering::SeqCst);
                self.current.job.store(job, Ordering::SeqCst);
                let response = self.transcribe(job, audio, language, &vocabulary);
                self.current.job.store(0, Ordering::SeqCst);
                response
            }
            // The reader thread acts on these; one can't reach the queue.
            Request::Cancel { .. } => Response::Error {
                job: None,
                message: "cancel is not queued".into(),
            },
        }
    }

    fn load(&mut self, model: &Path, arch: Arch, vad: &Path) -> Result<LoadedInfo, String> {
        let started = Instant::now();
        let model = Model::load(model, arch, self.use_gpu)?;
        let vad = Vad::load(vad)?;
        let info = LoadedInfo {
            arch: arch.as_str().into(),
            backend: model
                .gpu()
                .map_or_else(|| "CPU".into(), |d| d.backend.clone()),
            device: model.gpu().map_or_else(cpu_name, |d| d.description.clone()),
            on_gpu: model.gpu().is_some(),
            supports_vocabulary: arch.supports_vocabulary(),
            load_ms: started.elapsed().as_millis() as u64,
        };
        self.loaded = Some(Loaded { model, vad });
        Ok(info)
    }

    fn warm_up(&mut self) -> Response {
        let Some(loaded) = self.loaded.as_mut() else {
            return no_model(None);
        };
        let started = Instant::now();
        let silence = loaded.model.arch().warm_up_audio();
        let never = AtomicBool::new(false);
        match loaded.model.run(&silence, &RunOptions::default(), &never) {
            Ok(_) => Response::WarmedUp {
                ms: started.elapsed().as_millis() as u64,
            },
            Err(e) => Response::Error {
                job: None,
                message: format!("warm-up: {e}"),
            },
        }
    }

    fn transcribe(
        &mut self,
        job: u64,
        audio: &[f32],
        language: Option<String>,
        vocabulary: &[String],
    ) -> Response {
        let Some(loaded) = self.loaded.as_mut() else {
            return no_model(Some(job));
        };
        let abort = &self.current.abort;
        let vad_start = Instant::now();
        let speech = match loaded.vad.segments(audio, VAD_THRESHOLD, VAD_MIN_SPEECH_MS) {
            Ok(segments) => speech_range(&segments, audio.len(), SAMPLE_RATE),
            Err(e) => {
                return Response::Error {
                    job: Some(job),
                    message: format!("VAD: {e}"),
                };
            }
        };
        let vad_ms = vad_start.elapsed().as_millis() as u64;
        let Some((from, to)) = speech else {
            return Response::NoSpeech { job };
        };
        if abort.load(Ordering::SeqCst) {
            return Response::Aborted { job };
        }
        let options = RunOptions {
            language,
            prompt: vocabulary_prompt(vocabulary),
        };
        let asr_start = Instant::now();
        match loaded.model.run(&audio[from..to], &options, abort) {
            Ok(t) => Response::Transcript(TranscriptMsg {
                job,
                text: t.text,
                language: t.language,
                vad_ms,
                transcribe_ms: asr_start.elapsed().as_millis() as u64,
                speech_ms: (to - from) as u64 * 1000 / u64::from(SAMPLE_RATE),
            }),
            Err(RunError::Aborted) => Response::Aborted { job },
            Err(RunError::Failed(message)) => Response::Error {
                job: Some(job),
                message,
            },
        }
    }
}

fn no_model(job: Option<u64>) -> Response {
    Response::Error {
        job,
        message: "no model loaded".into(),
    }
}

fn cpu_name() -> String {
    native::devices()
        .into_iter()
        .find(|d| d.kind == DeviceKind::Cpu)
        .map_or_else(|| "CPU".into(), |d| d.description)
}

/// Whisper's initial prompt reads as preceding text, so a comma-separated list of the user's
/// terms nudges it toward those spellings.
fn vocabulary_prompt(vocabulary: &[String]) -> Option<String> {
    let terms: Vec<&str> = vocabulary
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    (!terms.is_empty()).then(|| format!("{}.", terms.join(", ")))
}

/// Decodes stdin until it closes. Exits the process when the app goes away or sends garbage.
fn read_stdin(queue: &Sender<Frame<Request>>, current: &Current) {
    let mut stdin = io::stdin().lock(); // already buffered
    loop {
        let frame = match recv_frame::<Request>(&mut stdin) {
            Ok(Some(frame)) => frame,
            // The app closed stdin (unload, quit, or it died).
            Ok(None) => std::process::exit(0),
            Err(e) => {
                eprintln!("engine worker: bad request stream: {e}");
                std::process::exit(1);
            }
        };
        if let Request::Cancel { job } = frame.message {
            if current.job.load(Ordering::SeqCst) == job {
                current.abort.store(true, Ordering::SeqCst);
            }
        } else if queue.send(frame).is_err() {
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_becomes_a_comma_separated_prompt() {
        let terms = vec!["Kubernetes".to_string(), "  ".into(), " Córdoba ".into()];
        assert_eq!(
            vocabulary_prompt(&terms).as_deref(),
            Some("Kubernetes, Córdoba.")
        );
        assert_eq!(vocabulary_prompt(&[]), None);
    }
}
