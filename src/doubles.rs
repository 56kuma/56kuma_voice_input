//! Test doubles (fakes) for every boundary trait.
//!
//! They live in the library so both unit tests and integration tests can use
//! them, and so the whole application logic can be exercised without a
//! microphone, a network connection, or a display.

use std::sync::{Arc, Mutex};

use crate::app::state::State;
use crate::audio::audio_data::AudioData;
use crate::audio::{AudioError, AudioRecorder};
use crate::config::credentials::{CredentialError, CredentialStore};
use crate::hotkey::{Hotkey, HotkeyError, HotkeyProvider};
use crate::input::{InputError, TextInjector};
use crate::transcription::{TranscriptionError, TranscriptionProvider};
use crate::ui::Overlay;

#[derive(Default)]
struct RecorderInner {
    recording: bool,
    start_count: usize,
    stop_count: usize,
    start_result: Option<AudioError>,
    stop_result: Option<AudioError>,
    audio: Option<AudioData>,
}

/// A microphone that records nothing and reports what was asked of it.
#[derive(Clone, Default)]
pub struct FakeAudioRecorder {
    inner: Arc<Mutex<RecorderInner>>,
}

impl FakeAudioRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Audio to hand back from `stop()`. Defaults to one second of silence.
    pub fn with_audio(self, audio: AudioData) -> Self {
        self.inner.lock().unwrap().audio = Some(audio);
        self
    }

    pub fn failing_to_start(self, err: AudioError) -> Self {
        self.inner.lock().unwrap().start_result = Some(err);
        self
    }

    pub fn failing_to_stop(self, err: AudioError) -> Self {
        self.inner.lock().unwrap().stop_result = Some(err);
        self
    }

    pub fn is_recording(&self) -> bool {
        self.inner.lock().unwrap().recording
    }

    pub fn start_count(&self) -> usize {
        self.inner.lock().unwrap().start_count
    }

    pub fn stop_count(&self) -> usize {
        self.inner.lock().unwrap().stop_count
    }
}

impl AudioRecorder for FakeAudioRecorder {
    fn start(&mut self) -> Result<(), AudioError> {
        let mut inner = self.inner.lock().unwrap();
        inner.start_count += 1;
        if let Some(err) = inner.start_result.clone() {
            return Err(err);
        }
        inner.recording = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<AudioData, AudioError> {
        let mut inner = self.inner.lock().unwrap();
        inner.stop_count += 1;
        if !inner.recording {
            return Err(AudioError::NotRecording);
        }
        inner.recording = false;
        if let Some(err) = inner.stop_result.clone() {
            return Err(err);
        }
        Ok(inner
            .audio
            .clone()
            .unwrap_or_else(|| AudioData::new(16_000, vec![0; 16_000])))
    }
}

/// A provider that returns a canned result and remembers what it received.
#[derive(Clone)]
pub struct FakeTranscriptionProvider {
    result: Result<String, TranscriptionError>,
    received: Arc<Mutex<Vec<AudioData>>>,
}

impl FakeTranscriptionProvider {
    pub fn returning(text: impl Into<String>) -> Self {
        Self {
            result: Ok(text.into()),
            received: Arc::default(),
        }
    }

    pub fn failing(err: TranscriptionError) -> Self {
        Self {
            result: Err(err),
            received: Arc::default(),
        }
    }

    pub fn received(&self) -> Vec<AudioData> {
        self.received.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TranscriptionProvider for FakeTranscriptionProvider {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        self.received.lock().unwrap().push(audio);
        self.result.clone()
    }
}

/// An injector that records every insertion instead of typing.
#[derive(Clone, Default)]
pub struct FakeTextInjector {
    inserted: Arc<Mutex<Vec<String>>>,
    failure: Option<InputError>,
}

impl FakeTextInjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn failing(err: InputError) -> Self {
        Self {
            inserted: Arc::default(),
            failure: Some(err),
        }
    }

    pub fn inserted(&self) -> Vec<String> {
        self.inserted.lock().unwrap().clone()
    }
}

impl TextInjector for FakeTextInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        if let Some(err) = &self.failure {
            return Err(err.clone());
        }
        self.inserted.lock().unwrap().push(text.to_owned());
        Ok(())
    }
}

/// An overlay that records the sequence of rendered states.
#[derive(Clone, Default)]
pub struct FakeOverlay {
    rendered: Arc<Mutex<Vec<State>>>,
}

impl FakeOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rendered(&self) -> Vec<State> {
        self.rendered.lock().unwrap().clone()
    }

    pub fn last(&self) -> Option<State> {
        self.rendered.lock().unwrap().last().copied()
    }
}

impl Overlay for FakeOverlay {
    fn render(&self, state: State) {
        self.rendered.lock().unwrap().push(state);
    }
}

/// In-memory credential store; `broken()` simulates a missing keyring.
#[derive(Clone, Default)]
pub struct FakeCredentialStore {
    secrets: Arc<Mutex<std::collections::HashMap<String, String>>>,
    broken: bool,
}

impl FakeCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(account: &str, secret: &str) -> Self {
        let store = Self::new();
        store.set(account, secret).unwrap();
        store
    }

    pub fn broken() -> Self {
        Self {
            broken: true,
            ..Self::default()
        }
    }
}

impl CredentialStore for FakeCredentialStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredentialError> {
        if self.broken {
            return Err(CredentialError::Unavailable("fake".into()));
        }
        Ok(self.secrets.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), CredentialError> {
        if self.broken {
            return Err(CredentialError::Unavailable("fake".into()));
        }
        self.secrets
            .lock()
            .unwrap()
            .insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        self.secrets.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A hotkey provider you can "press" from a test.
#[derive(Default)]
pub struct FakeHotkeyProvider {
    on_toggle: Option<Box<dyn Fn() + Send + Sync>>,
    fail: bool,
}

impl FakeHotkeyProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulates an OS that refuses the registration.
    pub fn failing() -> Self {
        Self {
            on_toggle: None,
            fail: true,
        }
    }

    /// Simulates one press of the registered hotkey.
    pub fn press(&self) {
        if let Some(cb) = &self.on_toggle {
            cb();
        }
    }
}

impl HotkeyProvider for FakeHotkeyProvider {
    fn register(
        &mut self,
        hotkey: &Hotkey,
        on_toggle: Box<dyn Fn() + Send + Sync>,
    ) -> Result<(), HotkeyError> {
        if self.fail {
            return Err(HotkeyError::Register(hotkey.to_string(), "fake".into()));
        }
        self.on_toggle = Some(on_toggle);
        Ok(())
    }
}
