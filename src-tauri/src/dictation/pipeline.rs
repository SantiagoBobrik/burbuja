//! What happens after a recording: transcribe with the local engine,
//! strip markers, optional local LLM cleanup, replacements, insertion. Runs as one async task
//! per job; the controller cancels it with the job's token (and by aborting the task).

use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::cleanup::{Cleanup, LlmError};
use crate::core::text::{apply_replacements, clean_transcript};
use crate::engine::{EngineError, EngineHost, Outcome, models};
use crate::events::ErrorCode;
use crate::insert::{self, Inserted};
use crate::settings::Settings;

pub struct Input {
    pub job: u64,
    pub samples: Vec<f32>,
    pub settings: Settings,
    pub engine: Arc<EngineHost>,
    /// For the local LLM server (see `cleanup::local_client`).
    pub client: reqwest::Client,
    pub cancel: CancellationToken,
    /// Called once when the LLM cleanup starts ("Polishing").
    pub on_polishing: Box<dyn FnOnce() + Send>,
}

/// Stage durations of one job, in ms (`None` when the stage did not run).
#[derive(Debug, Clone, Copy, Default)]
pub struct Stages {
    pub vad_ms: f64,
    pub transcribe_ms: f64,
    pub cleanup_ms: Option<f64>,
    pub insert_ms: f64,
}

pub enum PipelineOutcome {
    Done {
        text: String,
        stages: Stages,
        /// Why the AI cleanup failed, when it did (the raw text was inserted).
        cleanup_failed: Option<ErrorCode>,
        target_app: Option<String>,
        raw: String,
        engine: String,
        inserted: Inserted,
    },
    Failed(ErrorCode, String),
    Aborted,
}

/// A finished transcription.
struct Transcript {
    text: String,
    language: Option<String>,
    vad_ms: f64,
    transcribe_ms: f64,
}

pub async fn run(input: Input) -> PipelineOutcome {
    let Input {
        job,
        samples,
        settings,
        engine,
        client,
        cancel,
        on_polishing,
    } = input;

    // 1. Transcribe.
    let transcript = match transcribe(&engine, job, samples, &settings).await {
        Ok(t) => t,
        Err((code, message)) => return PipelineOutcome::Failed(code, message.into()),
    };
    let Some(transcript) = transcript else {
        return PipelineOutcome::Aborted;
    };
    let mut stages = Stages {
        vad_ms: transcript.vad_ms,
        transcribe_ms: transcript.transcribe_ms,
        ..Stages::default()
    };
    let raw_clean = clean_transcript(&transcript.text);
    if raw_clean.is_empty() {
        return PipelineOutcome::Failed(ErrorCode::NoSpeech, "Nothing to insert".into());
    }

    // 2. Cleanup (falls back to the raw text on any failure).
    let mut text = raw_clean.clone();
    let mut cleanup_failed = None;
    if let Some(cleanup) = Cleanup::from_settings(&settings, &client) {
        on_polishing();
        let cleanup_started = Instant::now();
        let spanish = cleanup.wants_spanish(transcript.language.as_deref());
        match cleanup.polish(&raw_clean, spanish, cancel.clone()).await {
            Ok(Some(cleaned)) => text = cleaned,
            Ok(None) => {}
            Err(LlmError::Cancelled) => return PipelineOutcome::Aborted,
            Err(e) => cleanup_failed = Some(e.code()),
        }
        stages.cleanup_ms = Some(ms(cleanup_started));
    }

    // 3. Replacements after cleanup.
    let text = apply_replacements(&text, &settings.replacements);
    if text.trim().is_empty() {
        return PipelineOutcome::Failed(ErrorCode::NoSpeech, "Nothing to insert".into());
    }
    if cancel.is_cancelled() {
        return PipelineOutcome::Aborted;
    }

    // 4. Insert (blocking: waits for modifiers, sleeps before restoring the clipboard).
    let (method, restore) = (settings.insert_method, settings.restore_clipboard);
    let to_insert = text.clone();
    let insert_started = Instant::now();
    let (inserted, target_app) = tokio::task::spawn_blocking(move || {
        let app = insert::target_app();
        (insert::insert(&to_insert, method, restore), app)
    })
    .await
    .unwrap_or((Inserted::ClipboardOnly, None));
    stages.insert_ms = ms(insert_started);
    PipelineOutcome::Done {
        text,
        stages,
        cleanup_failed,
        target_app,
        raw: raw_clean,
        engine: settings.active_model.clone(),
        inserted,
    }
}

/// Runs the local engine. `Ok(None)`: the job was aborted. `Err`: what to tell the user.
async fn transcribe(
    engine: &Arc<EngineHost>,
    job: u64,
    samples: Vec<f32>,
    settings: &Settings,
) -> Result<Option<Transcript>, (ErrorCode, &'static str)> {
    let started = Instant::now();
    match engine
        .transcribe(
            job,
            samples,
            language_hint(settings),
            settings.vocabulary.clone(),
        )
        .await
    {
        Ok(Outcome::Text(t)) => {
            log::info!(
                "engine vad={}ms speech={}ms transcribe={}ms",
                t.vad_ms,
                t.speech_ms,
                t.transcribe_ms
            );
            // Wall time, so the stages add up: IPC and decoding count as transcription.
            let vad_ms = t.vad_ms as f64;
            Ok(Some(Transcript {
                text: t.text,
                language: t.language,
                vad_ms,
                transcribe_ms: (ms(started) - vad_ms).max(0.0),
            }))
        }
        Ok(Outcome::NoSpeech) => Err((ErrorCode::NoSpeech, "No speech heard")),
        Ok(Outcome::Aborted) => Ok(None),
        Err(EngineError::ModelMissing) => {
            Err((ErrorCode::ModelMissing, "Download a model in Settings"))
        }
        Err(e) => {
            log::warn!("transcription failed: {e}");
            Err((ErrorCode::EngineCrashed, "Transcription failed. Try again"))
        }
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// `auto` lets the model detect the language; English-only models (not `multilingual` in the
/// catalog) are forced to `en`.
pub fn language_hint(settings: &Settings) -> Option<String> {
    if models::find(&settings.active_model).is_some_and(|m| !m.multilingual) {
        Some("en".into())
    } else if settings.language.eq_ignore_ascii_case("auto") || settings.language.is_empty() {
        None
    } else {
        Some(settings.language.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_hint_rules() {
        let mut s = Settings::default();
        assert_eq!(language_hint(&s), None);
        s.language = "es".into();
        assert_eq!(language_hint(&s).as_deref(), Some("es"));
        s.active_model = "whisper-base.en".into();
        assert_eq!(language_hint(&s).as_deref(), Some("en"));
    }
}
