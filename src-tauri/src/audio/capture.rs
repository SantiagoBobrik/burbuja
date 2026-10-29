//! Microphone capture with `cpal`, delivered as 16 kHz mono f32.
//!
//! Privacy: the device is opened only for a recording or the settings meter and closed as soon
//! as it ends. The device is chosen by name; a missing name falls back to the system default.
//!
//! Each open microphone owns one thread, which also owns the `cpal` stream (streams aren't
//! `Send` on every platform). The audio callback only converts, downmixes and resamples into a
//! fresh buffer and sends it over a channel: no locks, no waiting. The thread meters the audio,
//! keeps it if recording, and closes the stream when told to stop.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig};

use super::levels::{BANDS, LevelMeter};
use crate::core::resample::{Resampler, mix_to_mono};

pub const SAMPLE_RATE: u32 = 16_000;
/// Level updates are computed over 30 ms of audio.
const LEVEL_BLOCK: usize = SAMPLE_RATE as usize * 30 / 1000;
/// How long the audio backend may take to open the device.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MicInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MicError {
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("no microphone connected")]
    NoDevice,
    #[error("microphone unavailable: {0}")]
    Unavailable(String),
}

impl From<cpal::Error> for MicError {
    fn from(e: cpal::Error) -> Self {
        match e.kind() {
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::Unavailable(e.to_string()),
        }
    }
}

/// What a finished recording hands to the pipeline.
#[derive(Debug, Default)]
pub struct Recording {
    pub samples: Vec<f32>,
    pub peak: f32,
    pub duration: Duration,
    pub device: String,
}

/// Called per level block with the smoothed bands and the raw peak since the stream opened.
pub type LevelCallback = Box<dyn FnMut([f32; BANDS], f32) + Send>;
type FailureCallback = Box<dyn FnOnce(String) + Send>;

enum Msg {
    Audio(Vec<f32>),
    Stop,
}

/// What the capture thread returns when it ends.
#[derive(Default)]
struct Captured {
    samples: Vec<f32>,
    peak: f32,
}

/// An open, running microphone. Dropping it closes the device and discards the audio.
pub struct Mic {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<Captured>>,
    device: String,
    started: Instant,
}

impl Mic {
    /// Opens `device_name` (`""` = system default) and starts capturing. `collect = false` is the
    /// settings meter: levels only, no audio kept. `used_fallback` reports that the named device
    /// was missing and the default was used instead.
    pub fn start(
        device_name: &str,
        collect: bool,
        on_level: LevelCallback,
        on_failure: impl FnOnce(String) + Send + 'static,
    ) -> Result<(Mic, bool), MicError> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let wanted = device_name.to_string();
        let audio_tx = tx.clone();
        let on_failure: FailureCallback = Box::new(on_failure);
        let thread = thread::Builder::new()
            .name("mic".into())
            .spawn(move || {
                let stream = match open(&wanted, audio_tx, on_failure) {
                    Ok((stream, name, fallback)) => {
                        let _ = ready_tx.send(Ok((name, fallback)));
                        stream
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return Captured::default();
                    }
                };
                let captured = consume(&rx, collect, on_level);
                drop(stream); // closes the device
                captured
            })
            .map_err(|e| MicError::Unavailable(e.to_string()))?;
        let (device, used_fallback) = ready_rx
            .recv()
            .map_err(|_| MicError::Unavailable("capture thread ended".into()))??;
        Ok((
            Mic {
                tx,
                thread: Some(thread),
                device,
                started: Instant::now(),
            },
            used_fallback,
        ))
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    /// Stops and closes the device, returning everything captured.
    pub fn stop(mut self) -> Recording {
        let duration = self.started.elapsed();
        let captured = self.finish();
        Recording {
            samples: captured.samples,
            peak: captured.peak,
            duration,
            device: std::mem::take(&mut self.device),
        }
    }

    fn finish(&mut self) -> Captured {
        let _ = self.tx.send(Msg::Stop);
        let Some(thread) = self.thread.take() else {
            return Captured::default();
        };
        thread.join().unwrap_or_else(|_| {
            log::warn!("microphone thread panicked");
            Captured::default()
        })
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Meters (and keeps, when `collect`) audio until `Stop`; audio sent after it is dropped.
fn consume(rx: &Receiver<Msg>, collect: bool, mut on_level: LevelCallback) -> Captured {
    let mut meter = LevelMeter::new();
    let mut samples = Vec::new();
    let mut pending: Vec<f32> = Vec::with_capacity(LEVEL_BLOCK * 2);
    let seconds = LEVEL_BLOCK as f32 / SAMPLE_RATE as f32;
    while let Ok(Msg::Audio(block)) = rx.recv() {
        if collect {
            samples.extend_from_slice(&block);
        }
        pending.extend_from_slice(&block);
        let whole = pending.len() / LEVEL_BLOCK * LEVEL_BLOCK;
        for slice in pending[..whole].as_chunks::<LEVEL_BLOCK>().0 {
            let bands = meter.process(slice, seconds);
            on_level(bands, meter.peak());
        }
        pending.drain(..whole);
    }
    // The last partial block still counts toward the peak.
    if !pending.is_empty() {
        meter.process(&pending, pending.len() as f32 / SAMPLE_RATE as f32);
    }
    Captured {
        samples,
        peak: meter.peak(),
    }
}

/// Opens and starts the input stream. Returns it with the device name and whether the named
/// device was missing.
fn open(
    wanted: &str,
    tx: Sender<Msg>,
    on_failure: FailureCallback,
) -> Result<(cpal::Stream, String, bool), MicError> {
    let host = cpal::default_host();
    let named = if wanted.is_empty() {
        None
    } else {
        host.input_devices()?
            .find(|d| device_name(d).as_deref() == Some(wanted))
    };
    let used_fallback = !wanted.is_empty() && named.is_none();
    let device = match named {
        Some(d) => d,
        None => host
            .default_input_device()
            .ok_or_else(|| MicError::Unavailable("no microphone found".into()))?,
    };
    let name = device_name(&device).unwrap_or_else(|| "microphone".into());
    let supported = device.default_input_config()?;
    let config = supported.config();

    // A disconnect mid-recording ends it once; glitches are only logged.
    let on_failure = Arc::new(Mutex::new(Some(on_failure)));
    let on_error = move |e: cpal::Error| match e.kind() {
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => {
            log::debug!("microphone: {e}");
        }
        _ => {
            log::warn!("microphone stream failed: {e}");
            if let Some(f) = on_failure.lock().ok().and_then(|mut f| f.take()) {
                f(e.to_string());
            }
        }
    };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, tx, on_error),
        SampleFormat::I16 => build::<i16>(&device, &config, tx, on_error),
        SampleFormat::U16 => build::<u16>(&device, &config, tx, on_error),
        SampleFormat::I32 => build::<i32>(&device, &config, tx, on_error),
        SampleFormat::U32 => build::<u32>(&device, &config, tx, on_error),
        SampleFormat::I8 => build::<i8>(&device, &config, tx, on_error),
        SampleFormat::U8 => build::<u8>(&device, &config, tx, on_error),
        other => {
            return Err(MicError::Unavailable(format!(
                "unsupported sample format {other}"
            )));
        }
    }?;
    stream.play()?;
    log::info!(
        "microphone open: {} Hz, {} channel(s), {}",
        config.sample_rate,
        config.channels,
        supported.sample_format()
    );
    Ok((stream, name, used_fallback))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    tx: Sender<Msg>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut resampler = Resampler::new(config.sample_rate, SAMPLE_RATE);
    let mut interleaved: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    device.build_input_stream(
        *config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            interleaved.clear();
            interleaved.extend(data.iter().map(|s| s.to_sample::<f32>()));
            mix_to_mono(&interleaved, channels, &mut mono);
            let mut out = Vec::with_capacity(mono.len() / 2 + 2);
            resampler.process(&mono, &mut out);
            if !out.is_empty() {
                let _ = tx.send(Msg::Audio(out));
            }
        },
        on_error,
        Some(OPEN_TIMEOUT),
    )
}

fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

/// Input devices by name, default first, without duplicates.
pub fn list_microphones() -> Result<Vec<MicInfo>, MicError> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| device_name(&d));
    let mut mics: Vec<MicInfo> = Vec::new();
    for name in host.input_devices()?.filter_map(|d| device_name(&d)) {
        if !mics.iter().any(|m| m.name == name) {
            mics.push(MicInfo {
                is_default: default.as_deref() == Some(name.as_str()),
                name,
            });
        }
    }
    mics.sort_by_key(|m| !m.is_default);
    Ok(mics)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs microphones: records 0.7 s from each one and prints its peak (0 = digital silence).
    #[test]
    #[ignore = "uses the real microphones"]
    fn records_from_every_microphone() {
        for info in list_microphones().expect("list") {
            let on_level: LevelCallback = Box::new(|_, _| {});
            let (mic, fallback) =
                Mic::start(&info.name, true, on_level, |e| panic!("{e}")).expect("open");
            assert!(!fallback, "{} not found by name", info.name);
            thread::sleep(Duration::from_millis(700));
            let rec = mic.stop();
            let seconds = rec.samples.len() as f32 / SAMPLE_RATE as f32;
            println!("{}: {seconds:.2} s, peak {:.4}", rec.device, rec.peak);
            assert!((0.5..0.9).contains(&seconds), "{seconds} s");
        }
    }
}
