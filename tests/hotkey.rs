//! Hotkey wiring as done in main.rs, with a fake provider instead of the OS.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use voice_input::app::controller::{Controller, Dependencies};
use voice_input::app::state::State;
use voice_input::doubles::{
    FakeAudioRecorder, FakeHotkeyProvider, FakeOverlay, FakeTextInjector, FakeTranscriptionProvider,
};
use voice_input::hotkey::{Debouncer, Hotkey, HotkeyError, HotkeyProvider};

fn controller() -> (Controller, FakeAudioRecorder) {
    let recorder = FakeAudioRecorder::new();
    let controller = Controller::new(Dependencies {
        recorder: Box::new(recorder.clone()),
        provider: Arc::new(FakeTranscriptionProvider::returning("x")),
        injector: Box::new(FakeTextInjector::new()),
        overlay: Box::new(FakeOverlay::new()),
        error_display: Duration::from_secs(3),
    });
    (controller, recorder)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_burst_of_hotkey_events_starts_recording_exactly_once() {
    // Arrange: same wiring as main.rs, but the clock is injected.
    let (mut controller, recorder) = controller();
    let events = controller.sender();
    let clock = Arc::new(Mutex::new(Instant::now()));
    let debouncer = Mutex::new(Debouncer::new(Duration::from_millis(250)));
    let mut provider = FakeHotkeyProvider::new();
    let clock_for_cb = Arc::clone(&clock);
    provider
        .register(
            &Hotkey::default_toggle(),
            Box::new(move || {
                if debouncer
                    .lock()
                    .unwrap()
                    .accept(*clock_for_cb.lock().unwrap())
                {
                    events.hotkey_toggled();
                }
            }),
        )
        .unwrap();

    // Act: key auto-repeat delivers three presses within 30 ms.
    for _ in 0..3 {
        provider.press();
        *clock.lock().unwrap() += Duration::from_millis(10);
    }
    controller.settle().await;

    // Assert
    assert_eq!(controller.state(), State::Recording);
    assert_eq!(recorder.start_count(), 1);
    assert_eq!(
        recorder.stop_count(),
        0,
        "the repeat must not stop the recording"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_deliberate_second_press_after_the_window_stops_recording() {
    let (mut controller, recorder) = controller();
    let events = controller.sender();
    let clock = Arc::new(Mutex::new(Instant::now()));
    let debouncer = Mutex::new(Debouncer::new(Duration::from_millis(250)));
    let mut provider = FakeHotkeyProvider::new();
    let clock_for_cb = Arc::clone(&clock);
    provider
        .register(
            &Hotkey::default_toggle(),
            Box::new(move || {
                if debouncer
                    .lock()
                    .unwrap()
                    .accept(*clock_for_cb.lock().unwrap())
                {
                    events.hotkey_toggled();
                }
            }),
        )
        .unwrap();

    provider.press();
    *clock.lock().unwrap() += Duration::from_secs(2);
    provider.press();
    controller.settle().await;

    assert_eq!(recorder.start_count(), 1);
    assert_eq!(recorder.stop_count(), 1);
}

#[test]
fn registration_failure_is_reported_not_swallowed() {
    let mut provider = FakeHotkeyProvider::failing();

    let result = provider.register(&Hotkey::default_toggle(), Box::new(|| {}));

    assert!(matches!(result, Err(HotkeyError::Register(_, _))));
}
