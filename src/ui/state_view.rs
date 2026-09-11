//! Pure presentation logic for the overlay: which glyph to draw, whether
//! the window needs to animate, and the silver-ratio layout numbers.
//!
//! No egui types here so all of it is unit-testable.

use std::time::{Duration, Instant};

use crate::app::state::State;

/// `1 : √2` — the silver ratio used throughout the layout.
pub const SILVER: f32 = std::f32::consts::SQRT_2;

/// How long the ✓ is shown after a successful insertion.
pub const COMPLETE_FLASH: Duration = Duration::from_millis(1200);

/// One breath of the recording pulse.
pub const BREATH_PERIOD: Duration = Duration::from_millis(2400);

/// The only visual vocabulary the overlay has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// ○ Idle
    Ring,
    /// ● Recording
    Dot,
    /// ••• Transcribing / Inserting
    Dots,
    /// ✓ Complete (brief)
    Check,
    /// ! Error
    Bang,
}

/// Tracks the current state plus the short "complete" flash.
#[derive(Debug)]
pub struct StateView {
    state: State,
    complete_until: Option<Instant>,
}

impl Default for StateView {
    fn default() -> Self {
        Self {
            state: State::Idle,
            complete_until: None,
        }
    }
}

impl StateView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Record a state change observed at `now`.
    pub fn apply(&mut self, next: State, now: Instant) {
        self.complete_until = match (self.state, next) {
            (State::Inserting, State::Idle) => Some(now + COMPLETE_FLASH),
            _ => None,
        };
        self.state = next;
    }

    fn flashing(&self, now: Instant) -> bool {
        self.complete_until.is_some_and(|until| now < until)
    }

    pub fn glyph(&self, now: Instant) -> Glyph {
        match self.state {
            State::Idle if self.flashing(now) => Glyph::Check,
            State::Idle => Glyph::Ring,
            State::Recording => Glyph::Dot,
            State::Transcribing | State::Inserting => Glyph::Dots,
            State::Error => Glyph::Bang,
        }
    }

    /// `true` while something on screen is moving. Idle must return `false`
    /// so the UI thread can sleep (Idle CPU ≈ 0%).
    pub fn needs_animation(&self, now: Instant) -> bool {
        match self.state {
            State::Recording | State::Transcribing | State::Inserting => true,
            State::Idle => self.flashing(now),
            State::Error => false,
        }
    }
}

/// Scale factor for the recording dot: a slow breath plus a small response
/// to the microphone level (`0.0..=1.0`). Never exceeds the silver ratio.
pub fn pulse_scale(elapsed: Duration, level: f32) -> f32 {
    // Breath: 0..1 following a raised cosine, so it eases at both ends.
    let phase = (elapsed.as_secs_f32() / BREATH_PERIOD.as_secs_f32()).fract();
    let breath = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    // Split the available headroom (1 → √2) in silver proportion:
    // the larger share follows the voice, the smaller share breathes.
    let headroom = SILVER - 1.0;
    let breath_share = headroom / (1.0 + SILVER);
    let level_share = headroom - breath_share;
    (1.0 + breath * breath_share + level.clamp(0.0, 1.0) * level_share).min(SILVER)
}

/// Silver-ratio layout derived from a single height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub corner_radius: f32,
    pub glyph_size: f32,
    pub padding: f32,
}

impl Layout {
    pub fn from_height(height: f32) -> Self {
        Self {
            width: height * SILVER,
            height,
            corner_radius: height / (SILVER * SILVER * SILVER),
            glyph_size: height / (SILVER * SILVER),
            padding: height / (SILVER * SILVER * SILVER * SILVER),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn each_state_has_one_glyph() {
        let now = t0();
        let cases = [
            (State::Idle, Glyph::Ring),
            (State::Recording, Glyph::Dot),
            (State::Transcribing, Glyph::Dots),
            (State::Inserting, Glyph::Dots),
            (State::Error, Glyph::Bang),
        ];
        for (state, glyph) in cases {
            let mut view = StateView::new();
            view.apply(state, now);
            assert_eq!(view.glyph(now), glyph, "{state:?}");
        }
    }

    #[test]
    fn idle_after_inserting_flashes_a_check_then_returns_to_ring() {
        let now = t0();
        let mut view = StateView::new();
        view.apply(State::Inserting, now);

        view.apply(State::Idle, now);

        assert_eq!(view.glyph(now), Glyph::Check);
        assert_eq!(view.glyph(now + COMPLETE_FLASH / 2), Glyph::Check);
        assert_eq!(view.glyph(now + COMPLETE_FLASH), Glyph::Ring);
    }

    #[test]
    fn idle_after_error_does_not_flash_a_check() {
        let now = t0();
        let mut view = StateView::new();
        view.apply(State::Error, now);

        view.apply(State::Idle, now);

        assert_eq!(view.glyph(now), Glyph::Ring);
    }

    #[test]
    fn a_new_recording_cancels_a_pending_check_flash() {
        let now = t0();
        let mut view = StateView::new();
        view.apply(State::Inserting, now);
        view.apply(State::Idle, now);

        view.apply(State::Recording, now);

        assert_eq!(view.glyph(now), Glyph::Dot);
    }

    #[test]
    fn only_recording_transcribing_and_the_flash_need_animation() {
        let now = t0();
        let mut view = StateView::new();
        assert!(!view.needs_animation(now), "idle must be static");

        view.apply(State::Recording, now);
        assert!(view.needs_animation(now));

        view.apply(State::Transcribing, now);
        assert!(view.needs_animation(now));

        view.apply(State::Error, now);
        assert!(!view.needs_animation(now), "error is static");

        view.apply(State::Idle, now);
        assert!(!view.needs_animation(now));

        view.apply(State::Inserting, now);
        view.apply(State::Idle, now);
        assert!(view.needs_animation(now), "flash is animated");
        assert!(
            !view.needs_animation(now + COMPLETE_FLASH),
            "then static again"
        );
    }

    #[test]
    fn pulse_breathes_between_one_and_the_silver_ratio() {
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for ms in (0..=4800).step_by(20) {
            for level in [0.0, 0.5, 1.0] {
                let s = pulse_scale(Duration::from_millis(ms), level);
                min = min.min(s);
                max = max.max(s);
            }
        }
        assert!((min - 1.0).abs() < 1e-3, "min {min}");
        assert!(max <= SILVER, "max {max}");
        assert!(max > 1.05, "the pulse must be visible: max {max}");
    }

    #[test]
    fn pulse_responds_to_microphone_level() {
        let t = Duration::from_millis(0);
        assert!(pulse_scale(t, 1.0) > pulse_scale(t, 0.0));
    }

    #[test]
    fn layout_follows_the_silver_ratio() {
        let l = Layout::from_height(44.0);

        assert!((l.width / l.height - SILVER).abs() < 1e-3);
        assert!(
            (l.glyph_size * SILVER * SILVER - l.height).abs() < 1e-3,
            "glyph = h/2"
        );
        assert!(
            (l.corner_radius * SILVER * SILVER * SILVER - l.height).abs() < 1e-3,
            "corner = h/(2√2)"
        );
        assert!(l.padding > 0.0 && l.padding < l.height / 2.0);
    }
}
