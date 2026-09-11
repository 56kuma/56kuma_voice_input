//! The application state machine.
//!
//! This module is *pure*: it takes an [`Event`], mutates its own [`State`],
//! and returns the list of [`Effect`]s the outside world must perform.
//! It never touches the OS, the network, or the screen, which is what makes
//! it fully unit-testable.

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
}

/// Something the state machine asks the outside world to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    StartRecording,
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
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
