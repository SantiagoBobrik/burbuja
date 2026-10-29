//! Guards applied after recording and after LLM cleanup.

use std::time::Duration;

/// Recordings shorter than this are not transcribed ("Too short").
pub const MIN_RECORDING: Duration = Duration::from_millis(300);
/// A peak below this is digital silence (a muted or sleeping mic).
pub const SILENT_PEAK: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingVerdict {
    Transcribe,
    TooShort,
    Silent,
}

/// The first two post-recording checks, in order. VAD comes after, in `audio/`.
pub fn check_recording(duration: Duration, peak: f32) -> RecordingVerdict {
    if duration < MIN_RECORDING {
        RecordingVerdict::TooShort
    } else if peak < SILENT_PEAK {
        RecordingVerdict::Silent
    } else {
        RecordingVerdict::Transcribe
    }
}

/// Returns the cleaned text if it is usable, or `None` to fall back to the raw transcript.
/// A cleanup much longer than what was said has probably answered instead of cleaned.
pub fn accept_cleanup(raw: &str, cleaned: &str) -> Option<String> {
    let cleaned = cleaned.trim();
    let limit = raw.chars().count() as f64 * 1.5 + 40.0;
    if cleaned.chars().count() as f64 > limit {
        return None;
    }
    Some(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_wins_over_silent() {
        assert_eq!(
            check_recording(Duration::from_millis(299), 0.0),
            RecordingVerdict::TooShort
        );
    }

    #[test]
    fn silent_mic_detected() {
        assert_eq!(
            check_recording(Duration::from_secs(2), 0.0009),
            RecordingVerdict::Silent
        );
    }

    #[test]
    fn normal_recording_transcribes() {
        assert_eq!(
            check_recording(Duration::from_millis(300), 0.2),
            RecordingVerdict::Transcribe
        );
    }

    #[test]
    fn cleanup_within_limit_is_used() {
        let raw = "um so we should meet on tuesday no wednesday";
        assert_eq!(
            accept_cleanup(raw, "  We should meet on Wednesday.\n").as_deref(),
            Some("We should meet on Wednesday.")
        );
    }

    #[test]
    fn cleanup_that_answers_is_rejected() {
        let raw = "what is the capital of france";
        let answer = "The capital of France is Paris. It has been the capital since the 10th century and is home to the Louvre.";
        assert_eq!(accept_cleanup(raw, answer), None);
    }

    #[test]
    fn limit_is_one_and_a_half_raw_plus_forty() {
        let raw = "x".repeat(20); // limit = 70
        assert!(accept_cleanup(&raw, &"y".repeat(70)).is_some());
        assert!(accept_cleanup(&raw, &"y".repeat(71)).is_none());
    }

    #[test]
    fn empty_cleanup_is_accepted_as_empty() {
        assert_eq!(accept_cleanup("um uh", "").as_deref(), Some(""));
    }
}
