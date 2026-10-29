//! Level meter for the bubble waveform and the settings meter.
//!
//! Each 30 ms capture block is split into 16 slices.
//! Each slice's RMS goes through a `sqrt` curve so quiet mics still move the bars, then a
//! fast-attack / ~120 ms release smoother.

pub const BANDS: usize = 16;
const RELEASE_SECONDS: f32 = 0.120;

#[derive(Debug, Clone)]
pub struct LevelMeter {
    bands: [f32; BANDS],
    peak: f32,
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl LevelMeter {
    pub fn new() -> Self {
        Self {
            bands: [0.0; BANDS],
            peak: 0.0,
        }
    }

    /// Highest absolute sample seen so far (raw, before the curve).
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Feeds one chunk of mono samples lasting `seconds`; returns the smoothed bands (0..=1).
    pub fn process(&mut self, samples: &[f32], seconds: f32) -> [f32; BANDS] {
        if let Some(p) = samples.iter().map(|s| s.abs()).reduce(f32::max) {
            self.peak = self.peak.max(p);
        }
        let decay = (-seconds.max(0.0) / RELEASE_SECONDS).exp();
        let slice = samples.len().div_ceil(BANDS).max(1);
        for (i, band) in self.bands.iter_mut().enumerate() {
            let part = samples
                .get(i * slice..((i + 1) * slice).min(samples.len()))
                .unwrap_or(&[]);
            let level = rms(part).sqrt().min(1.0);
            *band = if level >= *band {
                level
            } else {
                level.max(*band * decay)
            };
        }
        self.bands
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: f32 = 0.030;

    #[test]
    fn silence_is_zero() {
        let mut m = LevelMeter::new();
        assert_eq!(m.process(&[0.0; 480], CHUNK), [0.0; BANDS]);
        assert_eq!(m.peak(), 0.0);
    }

    #[test]
    fn attack_is_immediate_and_sqrt_curve_applies() {
        let mut m = LevelMeter::new();
        let bands = m.process(&[0.25; 480], CHUNK);
        assert!(
            bands.iter().all(|b| (b - 0.5).abs() < 1e-6),
            "sqrt(0.25) = 0.5"
        );
        assert_eq!(m.peak(), 0.25);
    }

    #[test]
    fn release_takes_about_120_ms() {
        let mut m = LevelMeter::new();
        m.process(&[1.0; 480], CHUNK);
        let after_one = m.process(&[0.0; 480], CHUNK)[0];
        assert!((after_one - (-0.25f32).exp()).abs() < 1e-5);
        let mut level = after_one;
        for _ in 0..3 {
            level = m.process(&[0.0; 480], CHUNK)[0];
        }
        assert!(level < 0.37, "about 1/e after 120 ms, got {level}");
    }

    #[test]
    fn bands_follow_the_chunk_shape() {
        let mut m = LevelMeter::new();
        let mut chunk = vec![0.0; 480];
        chunk[..30].fill(0.64);
        let bands = m.process(&chunk, CHUNK);
        assert!((bands[0] - 0.8).abs() < 1e-5);
        assert_eq!(bands[15], 0.0);
    }

    #[test]
    fn short_chunks_do_not_panic() {
        let mut m = LevelMeter::new();
        m.process(&[0.5; 3], CHUNK);
        m.process(&[], CHUNK);
    }
}
