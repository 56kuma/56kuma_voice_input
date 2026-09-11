//! Wires the pure [`StateMachine`] to the boundary traits.
//!
//! The controller is the only place that executes [`Effect`]s. It owns one
//! event channel: hotkey presses, async transcription results, and timers all
//! arrive as [`Event`]s and are handled in order on a single task.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use super::state::{Effect, Event, Failure, Stage, State, StateMachine};
use crate::audio::AudioRecorder;
use crate::input::TextInjector;
use crate::transcription::TranscriptionProvider;
use crate::ui::Overlay;

/// Cloneable handle for pushing events from other threads (hotkey listener).
#[derive(Clone)]
pub struct EventSender(UnboundedSender<Event>);

impl EventSender {
    pub fn send(&self, event: Event) {
        let _ = self.0.send(event);
    }

    pub fn hotkey_toggled(&self) {
        self.send(Event::HotkeyToggled);
    }
}

/// Everything the controller needs from the outside world.
pub struct Dependencies {
    pub recorder: Box<dyn AudioRecorder>,
    pub provider: Arc<dyn TranscriptionProvider>,
    pub injector: Box<dyn TextInjector>,
    pub overlay: Box<dyn Overlay>,
    /// How long the Error state stays on screen before returning to Idle.
    pub error_display: Duration,
}

pub struct Controller {
    machine: StateMachine,
    deps: Dependencies,
    tx: UnboundedSender<Event>,
    rx: UnboundedReceiver<Event>,
}

impl Controller {
    pub fn new(deps: Dependencies) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            machine: StateMachine::new(),
            deps,
            tx,
            rx,
        }
    }

    pub fn sender(&self) -> EventSender {
        EventSender(self.tx.clone())
    }

    pub fn state(&self) -> State {
        self.machine.state()
    }

    /// Handle one event synchronously, executing all resulting effects.
    pub fn dispatch(&mut self, event: Event) {
        let before = self.machine.state();
        let effects = self.machine.handle(event);
        let after = self.machine.state();
        if before != after {
            log::info!("state {before:?} -> {after:?}");
        }
        for effect in effects {
            self.perform(effect);
        }
    }

    /// Wait for the next event and handle it. Returns `false` once every
    /// sender has been dropped.
    pub async fn step(&mut self) -> bool {
        match self.rx.recv().await {
            Some(event) => {
                self.dispatch(event);
                true
            }
            None => false,
        }
    }

    /// Process events until the loop ends.
    pub async fn run(&mut self) {
        while self.step().await {}
    }

    /// Drain queued events, giving spawned tasks a chance to produce more.
    /// Intended for tests; returns when the queue stays empty.
    pub async fn settle(&mut self) {
        loop {
            // Let any task spawned by the previous effect make progress.
            tokio::task::yield_now().await;
            match self.rx.try_recv() {
                Ok(event) => self.dispatch(event),
                Err(_) => return,
            }
        }
    }

    fn perform(&mut self, effect: Effect) {
        match effect {
            Effect::StartRecording => {
                if let Err(err) = self.deps.recorder.start() {
                    self.fail(Stage::Recording, err.to_string());
                }
            }
            Effect::StopRecording => match self.deps.recorder.stop() {
                Ok(audio) if audio.is_empty() => {
                    self.fail(Stage::Recording, "no audio captured");
                }
                Ok(audio) => {
                    log::debug!("captured {:?} of audio", audio.duration());
                    self.enqueue(Event::RecordingFinished(audio));
                }
                Err(err) => self.fail(Stage::Recording, err.to_string()),
            },
            Effect::Transcribe(audio) => {
                let provider = Arc::clone(&self.deps.provider);
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    // `audio` is dropped at the end of this task: nothing is
                    // ever written to disk.
                    let event = match provider.transcribe(audio).await {
                        Ok(text) => Event::TranscriptionSucceeded(text),
                        Err(err) => Event::Failed(Failure::new(Stage::Transcription, err.to_string())),
                    };
                    let _ = tx.send(event);
                });
            }
            Effect::Insert(text) => match self.deps.injector.insert(&text) {
                Ok(()) => self.enqueue(Event::InsertionSucceeded),
                Err(err) => self.fail(Stage::Insertion, err.to_string()),
            },
            Effect::ScheduleIdleReset => {
                let tx = self.tx.clone();
                let delay = self.deps.error_display;
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = tx.send(Event::IdleResetElapsed);
                });
            }
            Effect::Render(state) => self.deps.overlay.render(state),
        }
    }

    /// Follow-up events are queued, never dispatched recursively, so the
    /// effects of one event always complete (in order) before the next
    /// event is handled.
    fn enqueue(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    fn fail(&mut self, stage: Stage, detail: impl Into<String>) {
        let failure = Failure::new(stage, detail);
        log::error!("{:?} failed: {}", failure.stage, failure.detail);
        self.enqueue(Event::Failed(failure));
    }
}
