//! Microphone audio to the engine's format: mono, 16 kHz.
//!
//! [`Resampler`] is streaming (feed it capture blocks of any size) and low-passes before
//! downsampling, so energy above 8 kHz doesn't fold back into the speech band as noise. The
//! filter is a windowed-sinc FIR; positions between filtered samples are linearly interpolated,
//! which is accurate once the signal is band-limited this far below the input rate.

use std::f64::consts::PI;

/// Filter length: enough for a sharp edge at 48 kHz, ~1.3 ms of delay, trivial cost.
const TAPS: usize = 63;
/// Passband edge as a fraction of the output Nyquist (7.2 kHz for 16 kHz output).
const CUTOFF: f64 = 0.9;

/// Averages interleaved frames of `channels` samples into `out` (cleared first).
pub fn mix_to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    match channels {
        0 => {}
        1 => out.extend_from_slice(interleaved),
        n => out.extend(
            interleaved
                .chunks_exact(n)
                .map(|frame| frame.iter().sum::<f32>() / n as f32),
        ),
    }
}

#[derive(Debug, Clone)]
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Low-pass taps; empty when no resampling or no filtering is needed.
    taps: Vec<f32>,
    /// The last `TAPS - 1` inputs, for filtering across block boundaries.
    history: Vec<f32>,
    /// Filtered samples not consumed yet; index 0 is the last one of the previous block.
    filtered: Vec<f32>,
    /// Next output position, in input samples, relative to `filtered[0]`.
    pos: f64,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        let step = f64::from(input_rate.max(1)) / f64::from(output_rate.max(1));
        let taps = if step > 1.0 {
            low_pass(CUTOFF / (2.0 * step))
        } else {
            Vec::new()
        };
        Self {
            step,
            history: vec![0.0; taps.len().saturating_sub(1)],
            taps,
            filtered: Vec::new(),
            pos: 0.0,
        }
    }

    /// Resamples one block, appending the output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.step == 1.0 {
            out.extend_from_slice(input);
            return;
        }
        self.filter(input);
        while self.pos + 1.0 < self.filtered.len() as f64 {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            out.push(self.filtered[i] + (self.filtered[i + 1] - self.filtered[i]) * frac);
            self.pos += self.step;
        }
        // Keep the last filtered sample to interpolate across the next block's start.
        let consumed = self.filtered.len() - 1;
        self.filtered.drain(..consumed);
        self.pos -= consumed as f64;
    }

    /// Appends `input`, low-passed, to `self.filtered`.
    fn filter(&mut self, input: &[f32]) {
        if self.taps.is_empty() {
            self.filtered.extend_from_slice(input);
            return;
        }
        let keep = self.history.len();
        self.history.extend_from_slice(input);
        self.filtered.extend(
            self.history
                .windows(self.taps.len())
                .map(|w| w.iter().zip(&self.taps).map(|(x, h)| x * h).sum::<f32>()),
        );
        let drop = self.history.len() - keep;
        self.history.drain(..drop);
    }
}

/// Blackman-windowed sinc low-pass with unity DC gain; `cutoff` in cycles per input sample.
fn low_pass(cutoff: f64) -> Vec<f32> {
    let mid = (TAPS - 1) as f64 / 2.0;
    let raw: Vec<f64> = (0..TAPS)
        .map(|n| {
            let x = n as f64 - mid;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * x).sin() / (PI * x)
            };
            let t = n as f64 / (TAPS - 1) as f64;
            let window = 0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos();
            sinc * window
        })
        .collect();
    let sum: f64 = raw.iter().sum();
    raw.iter().map(|h| (h / sum) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, seconds: f64) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn resample_all(input: &[f32], from: u32, block: usize) -> Vec<f32> {
        let mut r = Resampler::new(from, 16_000);
        let mut out = Vec::new();
        for chunk in input.chunks(block) {
            r.process(chunk, &mut out);
        }
        out
    }

    /// RMS of the second half, once the filter has settled.
    fn settled_rms(x: &[f32]) -> f32 {
        let tail = &x[x.len() / 2..];
        (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
    }

    #[test]
    fn mono_mix_averages_channels() {
        let mut out = Vec::new();
        mix_to_mono(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, &mut out);
        assert_eq!(out, [0.5, 0.5, 0.0]);
        mix_to_mono(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, [0.1, 0.2]);
    }

    #[test]
    fn sixteen_khz_passes_through_untouched() {
        let input = sine(440.0, 16_000, 0.1);
        assert_eq!(resample_all(&input, 16_000, 160), input);
    }

    #[test]
    fn output_length_follows_the_rate_ratio() {
        for rate in [44_100, 48_000, 96_000, 22_050, 8_000] {
            let out = resample_all(&vec![0.0; rate as usize], rate, 441);
            assert!(
                (out.len() as i64 - 16_000).abs() <= 2,
                "{rate} Hz → {} samples",
                out.len()
            );
        }
    }

    #[test]
    fn speech_band_keeps_its_level() {
        for rate in [44_100, 48_000] {
            let out = resample_all(&sine(1_000.0, rate, 0.5), rate, 480);
            let rms = settled_rms(&out);
            assert!(
                (rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02,
                "{rate}: {rms}"
            );
        }
    }

    #[test]
    fn content_above_8_khz_is_removed_not_aliased() {
        // 12 kHz would fold to 4 kHz without the low-pass.
        let out = resample_all(&sine(12_000.0, 48_000, 0.5), 48_000, 480);
        assert!(settled_rms(&out) < 0.01, "{}", settled_rms(&out));
    }

    #[test]
    fn block_size_does_not_change_the_result() {
        let input = sine(700.0, 48_000, 0.3);
        let one_shot = resample_all(&input, 48_000, input.len());
        let small = resample_all(&input, 48_000, 37);
        assert_eq!(one_shot.len(), small.len());
        for (a, b) in one_shot.iter().zip(&small) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
