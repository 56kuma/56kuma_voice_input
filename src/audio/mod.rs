//! Audio capture. `audio_data` is pure; `recorder` is the thin cpal adapter.

pub mod audio_data;
pub mod recorder;

use audio_data::AudioData;

/// Errors from the microphone boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("no input device available")]
    NoDevice,
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("recorder is not recording")]
    NotRecording,
    #[error("audio device error: {0}")]
    Device(String),
}

/// A microphone. Only active between `start` and `stop`; Idle means the
/// device is fully released (no stream, no callbacks, ~0% CPU).
pub trait AudioRecorder: Send {
    fn start(&mut self) -> Result<(), AudioError>;
    fn stop(&mut self) -> Result<AudioData, AudioError>;
}
