//! The application state machine.
//!
//! This module is *pure*: it takes an [`Event`], mutates its own [`State`],
//! and returns the list of [`Effect`]s the outside world must perform.
//! It never touches the OS, the network, or the screen, which is what makes
//! it fully unit-testable.

use crate::audio::audio_data::AudioData;

/// Observable application state (mirrored 1:1 in the overlay UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Transcribing,
    Inserting,
    Error,
}

/// Something that happened, reported to the state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The global hotkey (default Ctrl+Space) was toggled.
    HotkeyToggled,
    /// The recorder stopped and produced audio.
    RecordingFinished(AudioData),
    /// The provider returned text (possibly empty).
    TranscriptionSucceeded(String),
    /// Text was injected into the focused application.
    InsertionSucceeded,
    /// Any stage failed. The detail is for logs only, never for the UI.
    Failed(Failure),
    /// The error-display timer elapsed.
    IdleResetElapsed,
}

/// Which stage failed, plus a human-readable detail for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Recording,
    Transcription,
    Insertion,
}

impl Failure {
    pub fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self {
            stage,
            detail: detail.into(),
        }
    }
}

/// Something the state machine asks the outside world to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    StartRecording,
    /// Stop the recorder; the controller reports back with
    /// [`Event::RecordingFinished`] or [`Event::Failed`].
    StopRecording,
    /// Send audio to the transcription provider.
    Transcribe(AudioData),
    /// Type text at the current cursor position.
    Insert(String),
    /// Go back to Idle after the error has been shown for a moment.
    ScheduleIdleReset,
    Render(State),
}

#[derive(Debug, Default)]
pub struct StateMachine {
    state: State,
}

impl Default for State {
    fn default() -> Self {
        State::Idle
    }
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> State {
        self.state
    }

    fn transition(&mut self, next: State) -> Vec<Effect> {
        self.state = next;
        vec![Effect::Render(next)]
    }

    pub fn handle(&mut self, event: Event) -> Vec<Effect> {
        match (self.state, event) {
            (State::Idle, Event::HotkeyToggled) => {
                self.state = State::Recording;
                vec![Effect::StartRecording, Effect::Render(State::Recording)]
            }
            (State::Recording, Event::HotkeyToggled) => {
                self.state = State::Transcribing;
                vec![Effect::StopRecording, Effect::Render(State::Transcribing)]
            }
            (State::Transcribing, Event::RecordingFinished(audio)) => {
                vec![Effect::Transcribe(audio)]
            }
            (State::Transcribing, Event::TranscriptionSucceeded(text)) => {
                if text.trim().is_empty() {
                    self.transition(State::Idle)
                } else {
                    self.state = State::Inserting;
                    vec![Effect::Insert(text), Effect::Render(State::Inserting)]
                }
            }
            (State::Inserting, Event::InsertionSucceeded) => self.transition(State::Idle),
            (State::Recording | State::Transcribing | State::Inserting, Event::Failed(_)) => {
                self.state = State::Error;
                vec![Effect::Render(State::Error), Effect::ScheduleIdleReset]
            }
            (State::Error, Event::IdleResetElapsed) => self.transition(State::Idle),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_plus_hotkey_transitions_to_transcribing_and_stops_recorder() {
        // Arrange
        let mut sm = StateMachine::new();
        sm.handle(Event::HotkeyToggled);

        // Act
        let effects = sm.handle(Event::HotkeyToggled);

        // Assert
        assert_eq!(sm.state(), State::Transcribing);
        assert_eq!(
            effects,
            vec![Effect::StopRecording, Effect::Render(State::Transcribing)]
        );
    }

    #[test]
    fn finished_recording_is_sent_to_transcription() {
        // Arrange
        let mut sm = StateMachine::new();
        sm.handle(Event::HotkeyToggled);
        sm.handle(Event::HotkeyToggled);
        let audio = AudioData::new(16_000, vec![1, 2, 3]);

        // Act
        let effects = sm.handle(Event::RecordingFinished(audio.clone()));

        // Assert
        assert_eq!(sm.state(), State::Transcribing);
        assert_eq!(effects, vec![Effect::Transcribe(audio)]);
    }

    fn machine_in(state: State) -> StateMachine {
        let mut sm = StateMachine::new();
        match state {
            State::Idle => {}
            State::Recording => {
                sm.handle(Event::HotkeyToggled);
            }
            State::Transcribing => {
                sm.handle(Event::HotkeyToggled);
                sm.handle(Event::HotkeyToggled);
            }
            State::Inserting => {
                sm.handle(Event::HotkeyToggled);
                sm.handle(Event::HotkeyToggled);
                sm.handle(Event::TranscriptionSucceeded("x".into()));
            }
            State::Error => {
                sm.handle(Event::HotkeyToggled);
                sm.handle(Event::Failed(Failure::new(Stage::Recording, "boom")));
            }
        }
        assert_eq!(sm.state(), state, "test fixture could not reach {state:?}");
        sm
    }

    #[test]
    fn transcription_success_transitions_to_inserting_with_text() {
        let mut sm = machine_in(State::Transcribing);

        let effects = sm.handle(Event::TranscriptionSucceeded("hello".into()));

        assert_eq!(sm.state(), State::Inserting);
        assert_eq!(
            effects,
            vec![
                Effect::Insert("hello".into()),
                Effect::Render(State::Inserting)
            ]
        );
    }

    #[test]
    fn insertion_success_transitions_to_idle() {
        let mut sm = machine_in(State::Inserting);

        let effects = sm.handle(Event::InsertionSucceeded);

        assert_eq!(sm.state(), State::Idle);
        assert_eq!(effects, vec![Effect::Render(State::Idle)]);
    }

    #[test]
    fn empty_transcription_skips_insertion_and_returns_to_idle() {
        for text in ["", "   ", "\n\t"] {
            let mut sm = machine_in(State::Transcribing);

            let effects = sm.handle(Event::TranscriptionSucceeded(text.into()));

            assert_eq!(sm.state(), State::Idle, "text={text:?}");
            assert_eq!(effects, vec![Effect::Render(State::Idle)], "text={text:?}");
        }
    }

    #[test]
    fn failure_in_any_active_state_transitions_to_error_and_schedules_reset() {
        for (state, stage) in [
            (State::Recording, Stage::Recording),
            (State::Transcribing, Stage::Transcription),
            (State::Inserting, Stage::Insertion),
        ] {
            let mut sm = machine_in(state);

            let effects = sm.handle(Event::Failed(Failure::new(stage, "detail")));

            assert_eq!(sm.state(), State::Error, "from {state:?}");
            assert_eq!(
                effects,
                vec![Effect::Render(State::Error), Effect::ScheduleIdleReset],
                "from {state:?}"
            );
        }
    }

    #[test]
    fn error_returns_to_idle_when_reset_timer_elapses() {
        let mut sm = machine_in(State::Error);

        let effects = sm.handle(Event::IdleResetElapsed);

        assert_eq!(sm.state(), State::Idle);
        assert_eq!(effects, vec![Effect::Render(State::Idle)]);
    }

    #[test]
    fn hotkey_is_ignored_while_busy_so_nothing_starts_twice() {
        for state in [State::Transcribing, State::Inserting, State::Error] {
            let mut sm = machine_in(state);

            let effects = sm.handle(Event::HotkeyToggled);

            assert_eq!(sm.state(), state, "state must not change from {state:?}");
            assert!(effects.is_empty(), "no effects expected in {state:?}");
        }
    }

    #[test]
    fn stale_events_from_a_previous_cycle_are_ignored_in_idle() {
        let mut sm = machine_in(State::Idle);

        let effects = sm.handle(Event::TranscriptionSucceeded("late".into()));

        assert_eq!(sm.state(), State::Idle);
        assert!(effects.is_empty());
    }

    #[test]
    fn idle_plus_hotkey_transitions_to_recording() {
        // Arrange
        let mut sm = StateMachine::new();

        // Act
        let effects = sm.handle(Event::HotkeyToggled);

        // Assert
        assert_eq!(sm.state(), State::Recording);
        assert_eq!(
            effects,
            vec![Effect::StartRecording, Effect::Render(State::Recording)]
        );
    }
}
