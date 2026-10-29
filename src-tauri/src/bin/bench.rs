//! Transcription benchmark:
//! `cargo run --bin bench -- [--cpu] [--model <file.bin>] <clip.wav>...`
//!
//! Loads the model once (resident, like the worker), warms it up, then runs each clip 3 times
//! through the same VAD → trim → transcribe path as the app and prints the median latency.
//! A file name containing `parakeet` runs on the Parakeet engine. The native engine is always
//! built optimized, so a dev build measures the same engine speed.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use burbuja_lib::core::vad_trim::speech_range;
use burbuja_lib::engine::models;
use burbuja_lib::engine::native::{self, Arch, Model, RunOptions, SAMPLE_RATE, Vad};
use burbuja_lib::engine::worker::{VAD_MIN_SPEECH_MS, VAD_THRESHOLD};

const RUNS: usize = 3;

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut cpu = false;
    let mut model: Option<PathBuf> = None;
    let mut clips = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cpu" => cpu = true,
            "--model" => model = args.next().map(PathBuf::from),
            _ => clips.push(PathBuf::from(arg)),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let model = model.unwrap_or_else(|| root.join("target/models").join(models::CATALOG[0].file));
    let vad_path = root.join("resources/models").join(models::SILERO_VAD.file);
    let arch = if model.to_string_lossy().contains("parakeet") {
        Arch::Parakeet
    } else {
        Arch::Whisper
    };

    native::init(cpu);
    let started = Instant::now();
    let mut asr = Model::load(&model, arch, !cpu).expect("load model");
    let load_ms = started.elapsed().as_millis();
    let mut vad = Vad::load(&vad_path).expect("load VAD");
    println!(
        "model {} | {} | device {} | load {load_ms} ms",
        model.file_name().unwrap().to_string_lossy(),
        arch.as_str(),
        asr.gpu().map_or_else(
            || "CPU".to_string(),
            |d| format!("{} ({})", d.description, d.backend)
        ),
    );

    let never = AtomicBool::new(false);
    let started = Instant::now();
    asr.run(&arch.warm_up_audio(), &RunOptions::default(), &never)
        .expect("warm-up");
    println!("warm-up {} ms", started.elapsed().as_millis());

    for clip in clips {
        let pcm = read_wav_16k_mono(&clip);
        let audio_ms = pcm.len() as u64 * 1000 / u64::from(SAMPLE_RATE);
        let mut times = Vec::new();
        let mut text = String::new();
        for _ in 0..RUNS {
            let started = Instant::now();
            let segments = vad
                .segments(&pcm, VAD_THRESHOLD, VAD_MIN_SPEECH_MS)
                .expect("VAD");
            text = match speech_range(&segments, pcm.len(), SAMPLE_RATE) {
                Some((a, b)) => {
                    asr.run(&pcm[a..b], &RunOptions::default(), &never)
                        .expect("run")
                        .text
                }
                None => "<no speech>".into(),
            };
            times.push(started.elapsed().as_millis());
        }
        times.sort_unstable();
        println!(
            "{} | audio {audio_ms} ms | median {} ms (runs {:?})\n  {}",
            clip.file_name().unwrap().to_string_lossy(),
            times[RUNS / 2],
            times,
            text.trim()
        );
    }
}

/// Reads a 16-bit PCM WAV; the fixtures are already 16 kHz mono.
fn read_wav_16k_mono(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read wav");
    let mut i = 12;
    let (mut rate, mut channels) = (0u32, 0u16);
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        if id == b"fmt " {
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
        } else if id == b"data" {
            assert_eq!(
                (rate, channels),
                (16_000, 1),
                "{}: expected 16 kHz mono",
                path.display()
            );
            return body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
                .collect();
        }
        i += 8 + len + (len & 1);
    }
    panic!("{}: no data chunk", path.display());
}
