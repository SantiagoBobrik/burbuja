//! Plays the signature sounds on the default output device without blocking the caller.
//!
//! A dedicated thread owns the rodio device sink (it is not `Send`) and receives play requests
//! over a channel. Buffers are pre-rendered per volume, so playback has no decode or synth cost.

use std::collections::HashMap;
use std::num::NonZero;
use std::sync::mpsc::{Sender, channel};
use std::thread;

use super::synth::{SAMPLE_RATE, SoundKind, render};

enum Msg {
    Play(SoundKind),
    SetVolume(f32),
}

#[derive(Clone)]
pub struct SoundPlayer {
    tx: Sender<Msg>,
}

impl SoundPlayer {
    pub fn spawn(volume: f32) -> Self {
        let (tx, rx) = channel::<Msg>();
        thread::Builder::new()
            .name("burbuja-sound".into())
            .spawn(move || {
                let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                    Ok(sink) => sink,
                    Err(e) => {
                        log::warn!("sound output unavailable: {e}");
                        return;
                    }
                };
                sink.log_on_drop(false);
                let mut buffers = prerender(volume);
                for msg in rx {
                    match msg {
                        Msg::Play(kind) => {
                            let samples = buffers[&kind].clone();
                            sink.mixer().add(rodio::buffer::SamplesBuffer::new(
                                NonZero::<u16>::MIN,
                                NonZero::new(SAMPLE_RATE).expect("non-zero rate"),
                                samples,
                            ));
                        }
                        Msg::SetVolume(v) => buffers = prerender(v),
                    }
                }
            })
            .expect("spawn sound thread");
        Self { tx }
    }

    pub fn play(&self, kind: SoundKind) {
        let _ = self.tx.send(Msg::Play(kind));
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Msg::SetVolume(volume));
    }
}

fn prerender(volume: f32) -> HashMap<SoundKind, Vec<f32>> {
    [SoundKind::Start, SoundKind::Stop, SoundKind::Cancel]
        .into_iter()
        .map(|k| (k, render(k, volume)))
        .collect()
}
