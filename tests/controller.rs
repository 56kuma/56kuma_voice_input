//! End-to-end behaviour of the application logic using fakes only:
//! no microphone, no network, no display.

use std::sync::Arc;
use std::time::Duration;

use voice_input::app::controller::{Controller, Dependencies};
use voice_input::app::state::State;
use voice_input::audio::audio_data::AudioData;
use voice_input::audio::AudioError;
use voice_input::doubles::{
    FakeAudioRecorder, FakeOverlay, FakeTextInjector, FakeTranscriptionProvider,
};
use voice_input::input::InputError;
use voice_input::transcription::TranscriptionError;

const ERROR_DISPLAY: Duration = Duration::from_secs(3);

struct World {
    controller: Controller,
    recorder: FakeAudioRecorder,
    provider: FakeTranscriptionProvider,
    injector: FakeTextInjector,
    overlay: FakeOverlay,
}

impl World {
    fn new() -> Self {
        Self::build(
            FakeAudioRecorder::new(),
            FakeTranscriptionProvider::returning("hello world"),
            FakeTextInjector::new(),
        )
    }

    fn build(
        recorder: FakeAudioRecorder,
        provider: FakeTranscriptionProvider,
        injector: FakeTextInjector,
    ) -> Self {
        let overlay = FakeOverlay::new();
        let controller = Controller::new(Dependencies {
            recorder: Box::new(recorder.clone()),
            provider: Arc::new(provider.clone()),
            injector: Box::new(injector.clone()),
            overlay: Box::new(overlay.clone()),
            error_display: ERROR_DISPLAY,
        });
        Self {
            controller,
            recorder,
            provider,
            injector,
            overlay,
        }
    }

    fn press_hotkey(&mut self) {
        self.controller.sender().hotkey_toggled();
    }

    async fn settle(&mut self) {
        self.controller.settle().await;
    }

    /// Full happy-path cycle: press, (speak), press, wait for everything.
    async fn dictate(&mut self) {
        self.press_hotkey();
        self.settle().await;
        self.press_hotkey();
        self.settle().await;
    }
}

// ---- State -----------------------------------------------------------------

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn idle_plus_hotkey_starts_recording() {
    let mut w = World::new();

    w.press_hotkey();
    w.settle().await;

    assert_eq!(w.controller.state(), State::Recording);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn full_cycle_ends_in_idle_after_inserting() {
    let mut w = World::new();

    w.dictate().await;

    assert_eq!(w.controller.state(), State::Idle);
    assert_eq!(
        w.overlay.rendered(),
        vec![
            State::Recording,
            State::Transcribing,
            State::Inserting,
            State::Idle
        ]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn api_error_shows_error_then_returns_to_idle() {
    let mut w = World::build(
        FakeAudioRecorder::new(),
        FakeTranscriptionProvider::failing(TranscriptionError::Timeout),
        FakeTextInjector::new(),
    );

    w.dictate().await;
    assert_eq!(w.controller.state(), State::Error);
    assert_eq!(w.overlay.last(), Some(State::Error));

    tokio::time::advance(ERROR_DISPLAY + Duration::from_millis(1)).await;
    w.settle().await;

    assert_eq!(w.controller.state(), State::Idle);
    assert!(w.injector.inserted().is_empty());
}

// ---- Recording -------------------------------------------------------------

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn microphone_is_only_active_while_recording() {
    let mut w = World::new();
    assert!(!w.recorder.is_recording(), "idle: mic must be off");

    w.press_hotkey();
    w.settle().await;
    assert!(w.recorder.is_recording(), "recording: mic must be on");

    w.press_hotkey();
    w.settle().await;
    assert!(!w.recorder.is_recording(), "after stop: mic must be off");
    assert_eq!(w.recorder.start_count(), 1);
    assert_eq!(w.recorder.stop_count(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn stopping_the_recorder_produces_audio_for_the_provider() {
    let audio = AudioData::new(16_000, vec![7; 8_000]);
    let mut w = World::build(
        FakeAudioRecorder::new().with_audio(audio.clone()),
        FakeTranscriptionProvider::returning("x"),
        FakeTextInjector::new(),
    );

    w.dictate().await;

    assert_eq!(w.provider.received(), vec![audio]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn missing_microphone_is_an_error_not_a_crash() {
    let mut w = World::build(
        FakeAudioRecorder::new().failing_to_start(AudioError::NoDevice),
        FakeTranscriptionProvider::returning("x"),
        FakeTextInjector::new(),
    );

    w.press_hotkey();
    w.settle().await;

    assert_eq!(w.controller.state(), State::Error);
    assert!(w.provider.received().is_empty());
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn empty_recording_is_an_error_and_never_hits_the_api() {
    let mut w = World::build(
        FakeAudioRecorder::new().with_audio(AudioData::new(16_000, vec![])),
        FakeTranscriptionProvider::returning("x"),
        FakeTextInjector::new(),
    );

    w.dictate().await;

    assert_eq!(w.controller.state(), State::Error);
    assert!(w.provider.received().is_empty());
}

// ---- Transcription / Input -------------------------------------------------

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn provider_result_is_inserted_exactly_once() {
    let mut w = World::new();

    w.dictate().await;

    assert_eq!(w.injector.inserted(), vec!["hello world".to_string()]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn empty_transcription_inserts_nothing() {
    let mut w = World::build(
        FakeAudioRecorder::new(),
        FakeTranscriptionProvider::returning("   "),
        FakeTextInjector::new(),
    );

    w.dictate().await;

    assert!(w.injector.inserted().is_empty());
    assert_eq!(w.controller.state(), State::Idle);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn injection_failure_becomes_error_state() {
    let mut w = World::build(
        FakeAudioRecorder::new(),
        FakeTranscriptionProvider::returning("x"),
        FakeTextInjector::failing(InputError::Failed("no focus".into())),
    );

    w.dictate().await;

    assert_eq!(w.controller.state(), State::Error);
}

// ---- Hotkey ----------------------------------------------------------------

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn hotkey_events_while_transcribing_do_not_start_a_second_recording() {
    let mut w = World::new();

    w.press_hotkey();
    w.press_hotkey();
    w.press_hotkey(); // arrives while transcription is in flight
    w.press_hotkey();
    w.settle().await;

    assert_eq!(w.recorder.start_count(), 1);
    assert_eq!(w.injector.inserted().len(), 1);
    assert_eq!(w.controller.state(), State::Idle);
}
