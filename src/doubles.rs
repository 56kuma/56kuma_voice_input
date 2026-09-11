//! Test doubles (fakes) for every boundary trait.
//!
//! They live in the library so both unit tests and integration tests can use
//! them, and so the whole application logic can be exercised without a
//! microphone, a network connection, or a display.

use std::sync::{Arc, Mutex};

use crate::app::state::State;
use crate::audio::audio_data::AudioData;
use crate::audio::{AudioError, AudioRecorder};
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
