//! Hold-to-talk, double-tap hands-free, Esc to cancel.
//!
//! Pure state machine: no clocks, no timers, no I/O. Time arrives inside the input and the
//! controller performs the returned [`Action`]. Recording starts on the first press, so a double
//! tap loses nothing from the start of the utterance.

use std::time::{Duration, Instant};

/// Holds shorter than this count as a tap.
pub const TAP_MAX_HOLD: Duration = Duration::from_millis(250);
/// Default double-tap window (configurable 150–800 ms).
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
/// Default session limit (configurable 1–60 min).
pub const MAX_SESSION: Duration = Duration::from_secs(5 * 60);
/// The bubble warns "ending soon" this long before [`MAX_SESSION`].
pub const ENDING_SOON: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationState {
    Idle,
    Holding,
    AwaitingSecondTap,
    HandsFree,
    Processing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Pressed(Instant),
    Released(Instant),
    /// Another key was pressed while the hotkey was held: it was a shortcut, not dictation.
    Chord,
    Escape,
    /// The double-tap timer fired; carries the token from [`Gestures::tap_token`].
    TapTimedOut(u32),
    MaxDurationReached,
    /// Click on the idle bubble: record hands-free without a key press.
    StartHandsFree,
    ProcessingFinished,
    /// Recording failed to start, the app was disabled, etc.
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    StartRecording,
    /// Schedule `TapTimedOut(tap_token)` after the double-tap window.
    AwaitSecondTap,
    EnterHandsFree,
    /// Drop the recording without transcribing (accidental tap, chord).
    DiscardRecording,
    /// The user pressed Esc: drop the recording and say so.
    CancelRecording,
    FinishRecording,
    CancelProcessing,
}

#[derive(Debug, Clone)]
pub struct Gestures {
    state: DictationState,
    pressed_at: Option<Instant>,
    ignore_next_release: bool,
    tap_token: u32,
}

impl Default for Gestures {
    fn default() -> Self {
        Self::new()
    }
}

impl Gestures {
    pub fn new() -> Self {
        Self {
            state: DictationState::Idle,
            pressed_at: None,
            ignore_next_release: false,
            tap_token: 0,
        }
    }

    pub fn state(&self) -> DictationState {
        self.state
    }

    pub fn tap_token(&self) -> u32 {
        self.tap_token
    }

    pub fn is_recording(&self) -> bool {
        matches!(
            self.state,
            DictationState::Holding | DictationState::AwaitingSecondTap | DictationState::HandsFree
        )
    }

    pub fn step(&mut self, input: Input) -> Action {
        match input {
            Input::Pressed(now) => self.pressed(now),
            Input::Released(now) => self.released(now),
            Input::Chord => self.chord(),
            Input::Escape => self.escape(),
            Input::TapTimedOut(token) => self.tap_timed_out(token),
            Input::MaxDurationReached => self.max_duration_reached(),
            Input::StartHandsFree => {
                if self.state != DictationState::Idle {
                    return Action::None;
                }
                self.state = DictationState::HandsFree;
                self.ignore_next_release = false;
                Action::StartRecording
            }
            Input::ProcessingFinished => {
                if self.state == DictationState::Processing {
                    self.state = DictationState::Idle;
                }
                Action::None
            }
            Input::Reset => {
                self.state = DictationState::Idle;
                self.ignore_next_release = false;
                Action::None
            }
        }
    }

    fn pressed(&mut self, now: Instant) -> Action {
        match self.state {
            DictationState::Idle => {
                self.state = DictationState::Holding;
                self.pressed_at = Some(now);
                self.ignore_next_release = false;
                Action::StartRecording
            }
            DictationState::AwaitingSecondTap => {
                self.state = DictationState::HandsFree;
                self.ignore_next_release = true;
                Action::EnterHandsFree
            }
            DictationState::HandsFree => {
                self.state = DictationState::Processing;
                self.ignore_next_release = true;
                Action::FinishRecording
            }
            DictationState::Holding | DictationState::Processing => Action::None,
        }
    }

    fn released(&mut self, now: Instant) -> Action {
        if self.ignore_next_release {
            self.ignore_next_release = false;
            return Action::None;
        }
        if self.state != DictationState::Holding {
            return Action::None;
        }
        let held = self
            .pressed_at
            .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
        if held < TAP_MAX_HOLD {
            self.state = DictationState::AwaitingSecondTap;
            self.tap_token = self.tap_token.wrapping_add(1);
            return Action::AwaitSecondTap;
        }
        self.state = DictationState::Processing;
        Action::FinishRecording
    }

    fn tap_timed_out(&mut self, token: u32) -> Action {
        if self.state != DictationState::AwaitingSecondTap || token != self.tap_token {
            return Action::None;
        }
        self.state = DictationState::Idle;
        Action::DiscardRecording
    }

    fn chord(&mut self) -> Action {
        if !matches!(
            self.state,
            DictationState::Holding | DictationState::AwaitingSecondTap
        ) {
            return Action::None;
        }
        self.state = DictationState::Idle;
        self.ignore_next_release = true;
        Action::DiscardRecording
    }

    fn escape(&mut self) -> Action {
        if self.is_recording() {
            self.ignore_next_release = self.state == DictationState::Holding;
            self.state = DictationState::Idle;
            return Action::CancelRecording;
        }
        if self.state == DictationState::Processing {
            self.state = DictationState::Idle;
            return Action::CancelProcessing;
        }
        Action::None
    }

    fn max_duration_reached(&mut self) -> Action {
        if !self.is_recording() {
            return Action::None;
        }
        self.ignore_next_release = self.state == DictationState::Holding;
        self.state = DictationState::Processing;
        Action::FinishRecording
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action as A;
    use DictationState as S;

    struct Clock(Instant);

    impl Clock {
        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn setup() -> (Gestures, Clock) {
        (Gestures::new(), Clock(Instant::now()))
    }

    /// Press at 0, release at 100, press at 200, release at 260: hands-free.
    fn hands_free() -> (Gestures, Clock) {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Released(c.at(100)));
        g.step(Input::Pressed(c.at(200)));
        g.step(Input::Released(c.at(260)));
        (g, c)
    }

    // ---- gesture scenarios ----

    /// Pressed at 0, released after 1500 ms: processing.
    fn processing() -> (Gestures, Clock) {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Released(c.at(1500)));
        (g, c)
    }

    /// Pressed at 0, released at 100: awaiting the second tap.
    fn tapped() -> (Gestures, Clock) {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Released(c.at(100)));
        (g, c)
    }

    #[test]
    fn hold_press_starts_recording() {
        let (mut g, c) = setup();
        assert_eq!(g.step(Input::Pressed(c.at(0))), A::StartRecording);
    }

    #[test]
    fn hold_release_transcribes() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::Released(c.at(1500))), A::FinishRecording);
        assert_eq!(g.state(), S::Processing);
    }

    #[test]
    fn press_while_processing_ignored() {
        let (mut g, c) = processing();
        assert_eq!(g.step(Input::Pressed(c.at(1600))), A::None);
    }

    #[test]
    fn release_while_processing_ignored() {
        let (mut g, c) = processing();
        g.step(Input::Pressed(c.at(1600)));
        assert_eq!(g.step(Input::Released(c.at(1700))), A::None);
    }

    #[test]
    fn processing_finished_to_idle() {
        let (mut g, _) = processing();
        g.step(Input::ProcessingFinished);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn tap_waits_for_second_tap() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::Released(c.at(100))), A::AwaitSecondTap);
    }

    #[test]
    fn double_tap_to_hands_free() {
        let (mut g, c) = tapped();
        assert_eq!(g.step(Input::Pressed(c.at(250))), A::EnterHandsFree);
        assert_eq!(g.state(), S::HandsFree);
    }

    #[test]
    fn stale_tap_timer_ignored() {
        let (mut g, c) = tapped();
        let token = g.tap_token();
        g.step(Input::Pressed(c.at(250)));
        assert_eq!(g.step(Input::TapTimedOut(token)), A::None);
    }

    #[test]
    fn second_taps_release_ignored() {
        let (mut g, c) = tapped();
        g.step(Input::Pressed(c.at(250)));
        assert_eq!(g.step(Input::Released(c.at(320))), A::None);
    }

    #[test]
    fn third_tap_stops_hands_free() {
        let (mut g, c) = tapped();
        g.step(Input::Pressed(c.at(250)));
        g.step(Input::Released(c.at(320)));
        assert_eq!(g.step(Input::Pressed(c.at(9000))), A::FinishRecording);
    }

    #[test]
    fn third_taps_release_ignored() {
        let (mut g, c) = tapped();
        g.step(Input::Pressed(c.at(250)));
        g.step(Input::Released(c.at(320)));
        g.step(Input::Pressed(c.at(9000)));
        assert_eq!(g.step(Input::Released(c.at(9080))), A::None);
    }

    #[test]
    fn single_tap_times_out_discard() {
        let (mut g, _) = tapped();
        assert_eq!(
            g.step(Input::TapTimedOut(g.tap_token())),
            A::DiscardRecording
        );
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn esc_cancels_hands_free() {
        let (mut g, _) = hands_free();
        assert_eq!(g.step(Input::Escape), A::CancelRecording);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn esc_cancels_while_holding() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::Escape), A::CancelRecording);
    }

    #[test]
    fn release_after_esc_ignored() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Escape);
        assert_eq!(g.step(Input::Released(c.at(800))), A::None);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn esc_kills_transcription() {
        let (mut g, _) = processing();
        assert_eq!(g.step(Input::Escape), A::CancelProcessing);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn late_finish_after_cancel_is_harmless() {
        let (mut g, _) = processing();
        g.step(Input::Escape);
        g.step(Input::ProcessingFinished);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn chord_discards() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::Chord), A::DiscardRecording);
    }

    #[test]
    fn release_after_chord_ignored() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Chord);
        assert_eq!(g.step(Input::Released(c.at(500))), A::None);
    }

    #[test]
    fn five_minute_limit_transcribes() {
        let (mut g, _) = hands_free();
        assert_eq!(g.step(Input::MaxDurationReached), A::FinishRecording);
        assert_eq!(g.state(), S::Processing);
    }

    #[test]
    fn limit_while_holding() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::MaxDurationReached), A::FinishRecording);
    }

    #[test]
    fn eventual_release_ignored() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::MaxDurationReached);
        assert_eq!(g.step(Input::Released(c.at(400_000))), A::None);
    }

    #[test]
    fn esc_idle_after_limit_cancels_processing() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::MaxDurationReached);
        g.step(Input::Released(c.at(400_000)));
        assert_eq!(g.step(Input::Escape), A::CancelProcessing);
    }

    #[test]
    fn esc_when_idle_does_nothing() {
        assert_eq!(Gestures::new().step(Input::Escape), A::None);
    }

    // ---- Burbuja additions ----

    #[test]
    fn hold_exactly_at_tap_threshold_transcribes() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::Released(c.at(250))), A::FinishRecording);
    }

    #[test]
    fn chord_while_awaiting_second_tap_discards() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Released(c.at(100)));
        assert_eq!(g.step(Input::Chord), A::DiscardRecording);
        assert_eq!(g.state(), S::Idle);
    }

    #[test]
    fn chord_in_hands_free_or_idle_is_ignored() {
        let (mut g, _) = hands_free();
        assert_eq!(g.step(Input::Chord), A::None);
        assert_eq!(g.state(), S::HandsFree);
        assert_eq!(Gestures::new().step(Input::Chord), A::None);
    }

    #[test]
    fn each_tap_gets_a_new_token() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Released(c.at(100)));
        let first = g.tap_token();
        g.step(Input::TapTimedOut(first));
        g.step(Input::Pressed(c.at(1000)));
        g.step(Input::Released(c.at(1100)));
        assert_ne!(g.tap_token(), first);
        assert_eq!(
            g.step(Input::TapTimedOut(first)),
            A::None,
            "old token ignored"
        );
        assert_eq!(
            g.step(Input::TapTimedOut(g.tap_token())),
            A::DiscardRecording
        );
    }

    #[test]
    fn reset_returns_to_idle_and_clears_ignored_release() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        g.step(Input::Chord); // sets ignore-next-release
        g.step(Input::Reset);
        assert_eq!(g.state(), S::Idle);
        g.step(Input::Pressed(c.at(1000)));
        assert_eq!(g.step(Input::Released(c.at(2000))), A::FinishRecording);
    }

    #[test]
    fn start_hands_free_from_idle_records_in_hands_free() {
        let (mut g, c) = setup();
        assert_eq!(g.step(Input::StartHandsFree), A::StartRecording);
        assert_eq!(g.state(), S::HandsFree);
        assert_eq!(
            g.step(Input::Pressed(c.at(5000))),
            A::FinishRecording,
            "next tap ends it"
        );
        assert_eq!(g.step(Input::Released(c.at(5080))), A::None);
    }

    #[test]
    fn start_hands_free_is_ignored_unless_idle() {
        let (mut g, c) = setup();
        g.step(Input::Pressed(c.at(0)));
        assert_eq!(g.step(Input::StartHandsFree), A::None);
        assert_eq!(g.state(), S::Holding);
    }

    #[test]
    fn max_duration_when_idle_does_nothing() {
        assert_eq!(Gestures::new().step(Input::MaxDurationReached), A::None);
    }
}
