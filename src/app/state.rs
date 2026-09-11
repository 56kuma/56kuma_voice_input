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
