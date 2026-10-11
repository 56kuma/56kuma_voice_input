//! xAI (Grok) `/v1/stt` provider.
//!
//! Same multipart shape as OpenAI except the path and the rule that the
//! audio file comes last, which the shared `post_transcription` guarantees.

use super::openai::post_transcription;
use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::audio_data::AudioData;

pub const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";
pub const DEFAULT_MODEL: &str = "grok-voice-transcribe-2.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XaiConfig {
    pub base_url: String,
    pub model: String,
    /// Language code such as `"ja"`. `None` lets the API auto-detect.
    pub language: Option<String>,
    pub timeout_secs: u64,
}

impl Default for XaiConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            language: None,
            timeout_secs: 30,
        }
    }
}

impl XaiConfig {
    /// The non-file multipart fields for one request.
    pub fn request_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![("model", self.model.clone())];
        if let Some(language) = &self.language {
            fields.push(("language", language.clone()));
        }
        fields
    }

    pub fn endpoint(&self) -> String {
        format!("{}/stt", self.base_url.trim_end_matches('/'))
    }
}

pub struct XaiTranscriptionProvider {
    api_key: Option<String>,
    config: XaiConfig,
    client: reqwest::Client,
}

impl XaiTranscriptionProvider {
    pub fn new(api_key: Option<String>, config: XaiConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            api_key,
            config,
            client,
        }
    }
}

#[async_trait::async_trait]
impl TranscriptionProvider for XaiTranscriptionProvider {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        post_transcription(
            &self.client,
            &self.config.endpoint(),
            self.api_key.as_deref(),
            self.config.request_fields(),
            audio,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_fields_carry_model_and_optional_language() {
        assert_eq!(
            XaiConfig::default().request_fields(),
            vec![("model", DEFAULT_MODEL.to_owned())]
        );

        let cfg = XaiConfig {
            language: Some("ja".into()),
            ..Default::default()
        };
        assert_eq!(
            cfg.request_fields(),
            vec![
                ("model", DEFAULT_MODEL.to_owned()),
                ("language", "ja".to_owned()),
            ]
        );
    }

    #[test]
    fn endpoint_is_stt_and_tolerates_trailing_slash() {
        assert_eq!(XaiConfig::default().endpoint(), "https://api.x.ai/v1/stt");
        let cfg = XaiConfig {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        assert_eq!(cfg.endpoint(), "https://example.test/v1/stt");
    }

    #[tokio::test]
    async fn missing_api_key_fails_before_any_network_call() {
        let provider = XaiTranscriptionProvider::new(
            None,
            XaiConfig {
                base_url: "http://127.0.0.1:9".into(), // nothing listens here
                ..Default::default()
            },
        );

        let result = provider
            .transcribe(AudioData::new(16_000, vec![0; 160]))
            .await;

        assert_eq!(result, Err(TranscriptionError::MissingApiKey));
    }
}
