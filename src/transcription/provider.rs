use crate::audio::audio_data::AudioData;

/// Errors from the speech-to-text boundary. Messages never contain the API
/// key or the audio/text content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptionError {
    #[error("no API key configured")]
    MissingApiKey,
    #[error("API key rejected (unauthorized)")]
    Unauthorized,
    #[error("API rate limit or quota exceeded")]
    RateLimited,
    #[error("request timed out")]
    Timeout,
    #[error("network error: {0}")]
    Network(String),
    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("invalid response from API: {0}")]
    InvalidResponse(String),
}

/// A pay-as-you-go speech-to-text backend.
#[async_trait::async_trait]
pub trait TranscriptionProvider: Send + Sync {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError>;
}
