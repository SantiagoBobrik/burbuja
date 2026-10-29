//! Trimming silence around speech using VAD segments.

/// Margin kept before the first and after the last speech segment.
pub const TRIM_MARGIN_MS: u32 = 200;

/// Given speech segments as `(start_sample, end_sample)` in order, returns the sample range to
/// transcribe (first start − margin .. last end + margin, clamped), or `None` if there is no
/// speech at all.
pub fn speech_range(
    segments: &[(i64, i64)],
    total: usize,
    sample_rate: u32,
) -> Option<(usize, usize)> {
    let first = segments.iter().map(|s| s.0).min()?;
    let last = segments.iter().map(|s| s.1).max()?;
    let margin = i64::from(TRIM_MARGIN_MS) * i64::from(sample_rate) / 1000;
    let start = (first - margin).clamp(0, total as i64) as usize;
    let end = (last + margin).clamp(0, total as i64) as usize;
    (end > start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 16_000;

    #[test]
    fn no_segments_means_no_speech() {
        assert_eq!(speech_range(&[], 16_000, SR), None);
    }

    #[test]
    fn keeps_200_ms_margins() {
        // speech 1.0 s .. 2.0 s in a 3 s clip
        assert_eq!(
            speech_range(&[(16_000, 32_000)], 48_000, SR),
            Some((12_800, 35_200))
        );
    }

    #[test]
    fn spans_all_segments_and_clamps_to_the_clip() {
        assert_eq!(
            speech_range(&[(1_000, 4_000), (40_000, 47_900)], 48_000, SR),
            Some((0, 48_000))
        );
    }
}
