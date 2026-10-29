//! Signature "pop" sounds, synthesized to match the Web Audio pack the user picked.
//!
//! Each tone is a sine with an exponential frequency sweep, an exponential attack/decay envelope
//! and a lowpass biquad, mirroring `OscillatorNode` + `GainNode` + `BiquadFilterNode`.

use std::f64::consts::TAU;

pub const SAMPLE_RATE: u32 = 48_000;
/// Web Audio's floor for exponential ramps (they cannot reach 0).
const FLOOR: f64 = 0.0001;
/// `BiquadFilterNode` default Q.
const Q: f64 = 1.0;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum SoundKind {
    Start,
    Stop,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    pub start: f64,
    pub f0: f64,
    pub f1: f64,
    pub sweep: f64,
    pub attack: f64,
    pub dur: f64,
    pub peak: f64,
    pub cutoff: f64,
}

/// Positional on purpose: each call reads like a row of the sound table.
#[allow(clippy::too_many_arguments)]
const fn tone(
    start: f64,
    f0: f64,
    f1: f64,
    sweep: f64,
    attack: f64,
    dur: f64,
    peak: f64,
    cutoff: f64,
) -> Tone {
    Tone {
        start,
        f0,
        f1,
        sweep,
        attack,
        dur,
        peak,
        cutoff,
    }
}

/// The "Burbuja" pack. Values match the chosen Web Audio pack exactly.
pub fn tones(kind: SoundKind) -> &'static [Tone] {
    const START: [Tone; 1] = [tone(0.000, 420.0, 900.0, 0.07, 0.003, 0.13, 0.35, 2600.0)];
    const STOP: [Tone; 1] = [tone(0.000, 860.0, 400.0, 0.08, 0.003, 0.14, 0.33, 2400.0)];
    const CANCEL: [Tone; 2] = [
        tone(0.000, 300.0, 180.0, 0.10, 0.006, 0.12, 0.30, 1500.0),
        tone(0.110, 260.0, 160.0, 0.10, 0.006, 0.12, 0.22, 1400.0),
    ];
    match kind {
        SoundKind::Start => &START,
        SoundKind::Stop => &STOP,
        SoundKind::Cancel => &CANCEL,
    }
}

impl Tone {
    /// `freq(t) = f0 · (f1/f0)^(min(t, sweep)/sweep)`
    pub fn freq(&self, t: f64) -> f64 {
        self.f0 * (self.f1 / self.f0).powf(t.min(self.sweep) / self.sweep)
    }

    /// Exponential attack to `peak`, exponential decay to the floor at `dur`, then silence.
    pub fn gain(&self, t: f64) -> f64 {
        if t < 0.0 {
            0.0
        } else if t < self.attack {
            FLOOR * (self.peak / FLOOR).powf(t / self.attack)
        } else if t < self.dur {
            self.peak * (FLOOR / self.peak).powf((t - self.attack) / (self.dur - self.attack))
        } else {
            0.0
        }
    }

    /// Renders this tone (dry, filtered) into `out`, starting at `self.start`.
    fn render_into(&self, out: &mut [f64], sr: f64) {
        let mut filter = Biquad::lowpass(self.cutoff, Q, sr);
        let first = (self.start * sr).round() as usize;
        let mut phase = 0.0_f64;
        for (i, slot) in out.iter_mut().enumerate().skip(first) {
            let t = (i - first) as f64 / sr;
            let x = phase.sin() * self.gain(t);
            phase = (phase + TAU * self.freq(t) / sr) % TAU;
            *slot += filter.process(x);
        }
    }
}

/// Renders a sound at [`SAMPLE_RATE`]: sum of tones × volume, through a soft `tanh` limiter.
pub fn render(kind: SoundKind, volume: f32) -> Vec<f32> {
    let sr = SAMPLE_RATE as f64;
    let tones = tones(kind);
    // Tail: 10 ms after the last tone ends, for the filter to ring out.
    let end = tones.iter().map(|t| t.start + t.dur).fold(0.0, f64::max) + 0.010;
    let mut mix = vec![0.0_f64; (end * sr).ceil() as usize];
    for t in tones {
        t.render_into(&mut mix, sr);
    }
    let volume = f64::from(volume.clamp(0.0, 1.0));
    mix.into_iter()
        .map(|x| (x * volume).tanh() as f32)
        .collect()
}

/// RBJ lowpass biquad with Web Audio's Q-in-dB convention: `alpha = sin(w0) / (2·10^(Q/20))`
/// (Web Audio spec, `BiquadFilterNode`, "lowpass").
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn lowpass(cutoff: f64, q_db: f64, sr: f64) -> Self {
        let w0 = TAU * cutoff / sr;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * 10f64.powf(q_db / 20.0));
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// 16-bit PCM mono WAV, for the reference files in `docs/sounds/`.
pub fn to_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SoundKind; 3] = [SoundKind::Start, SoundKind::Stop, SoundKind::Cancel];

    fn peak(s: &[f32]) -> f32 {
        s.iter().fold(0.0, |m, x| m.max(x.abs()))
    }

    #[test]
    fn durations_match_the_table() {
        let ms = |k| render(k, 0.6).len() as f64 / SAMPLE_RATE as f64 * 1000.0;
        assert!((ms(SoundKind::Start) - 140.0).abs() < 1.0);
        assert!((ms(SoundKind::Stop) - 150.0).abs() < 1.0);
        assert!((ms(SoundKind::Cancel) - 240.0).abs() < 1.0);
    }

    #[test]
    fn no_clipping_and_audible() {
        for k in ALL {
            let s = render(k, 1.0);
            assert!(peak(&s) <= 1.0, "{k:?} clips");
            let energy: f64 = s.iter().map(|x| f64::from(*x).powi(2)).sum();
            assert!(energy > 1.0, "{k:?} is silent ({energy})");
        }
    }

    #[test]
    fn peak_is_near_the_tone_peak() {
        // Start: peak 0.35 into a lowpass at 2600 Hz; the sweep stays below the cutoff.
        let p = peak(&render(SoundKind::Start, 1.0));
        assert!((0.25..=0.36).contains(&p), "start peak {p}");
    }

    #[test]
    fn volume_scales_and_zero_is_silent() {
        assert_eq!(peak(&render(SoundKind::Stop, 0.0)), 0.0);
        assert!(peak(&render(SoundKind::Stop, 0.3)) < peak(&render(SoundKind::Stop, 0.6)));
    }

    #[test]
    fn envelope_and_sweep_follow_the_formulas() {
        let t = tones(SoundKind::Start)[0];
        assert!((t.gain(0.0) - 0.0001).abs() < 1e-12);
        assert!((t.gain(t.attack) - t.peak).abs() < 1e-9);
        assert!((t.gain(t.dur - 1e-9) - 0.0001).abs() < 1e-6);
        assert_eq!(t.gain(t.dur), 0.0);
        assert!((t.freq(0.0) - 420.0).abs() < 1e-9);
        assert!((t.freq(1.0) - 900.0).abs() < 1e-9);
        assert!(
            (t.freq(0.035) - (420.0f64 * 900.0).sqrt()).abs() < 1e-6,
            "geometric midpoint"
        );
    }

    #[test]
    fn cancel_second_tone_starts_at_110_ms() {
        let s = render(SoundKind::Cancel, 1.0);
        let at = |ms: usize| s[ms * SAMPLE_RATE as usize / 1000];
        assert!(at(235).abs() > 0.0, "second tone still sounding at 235 ms");
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = to_wav(&[0.0, 0.5, -0.5], SAMPLE_RATE);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(wav.len(), 44 + 6);
    }

    /// Writes the reference WAVs to `docs/sounds/` at the default volume.
    #[test]
    fn write_reference_wavs() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/sounds");
        std::fs::create_dir_all(&dir).unwrap();
        for (k, name) in ALL.into_iter().zip(["start", "stop", "cancel"]) {
            std::fs::write(
                dir.join(format!("{name}.wav")),
                to_wav(&render(k, 0.6), SAMPLE_RATE),
            )
            .unwrap();
        }
    }
}
