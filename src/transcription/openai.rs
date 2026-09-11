//! OpenAI `/v1/audio/transcriptions` provider.
//!
//! Pure parts (request fields, response parsing) are separated from the one
//! thin HTTP call so they can be unit-tested without a network.

use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::audio_data::AudioData;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-4o-mini-transcribe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub model: String,
    /// ISO-639-1 code such as `"ja"`. `None` lets the API auto-detect.
    pub language: Option<String>,
    pub timeout_secs: u64,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            language: None,
            timeout_secs: 30,
        }
    }
}

impl OpenAiConfig {
    /// The non-file multipart fields for one request.
    pub fn request_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("model", self.model.clone()),
            ("response_format", "json".to_owned()),
        ];
        if let Some(language) = &self.language {
            fields.push(("language", language.clone()));
        }
        fields
    }

    pub fn endpoint(&self) -> String {
        format!(
            "{}/audio/transcriptions",
            self.base_url.trim_end_matches('/')
        )
    }
}

/// Maps an HTTP status + body to the vendor-neutral result.
pub fn parse_response(status: u16, body: &str) -> Result<String, TranscriptionError> {
    match status {
        200..=299 => serde_json::from_str::<SuccessBody>(body)
            .map(|b| b.text)
            .map_err(|e| TranscriptionError::InvalidResponse(e.to_string())),
        401 | 403 => Err(TranscriptionError::Unauthorized),
        429 => Err(TranscriptionError::RateLimited),
        _ => {
            let message = serde_json::from_str::<ErrorBody>(body)
                .map(|b| b.error.message)
                .unwrap_or_else(|_| "unexpected response".to_owned());
            Err(TranscriptionError::Api { status, message })
        }
    }
}

#[derive(serde::Deserialize)]
struct SuccessBody {
    text: String,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(serde::Deserialize)]
struct ErrorDetail {
    message: String,
}

pub struct OpenAiTranscriptionProvider {
    api_key: Option<String>,
    config: OpenAiConfig,
    client: reqwest::Client,
}

impl OpenAiTranscriptionProvider {
    pub fn new(api_key: Option<String>, config: OpenAiConfig) -> Self {
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
impl TranscriptionProvider for OpenAiTranscriptionProvider {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        let api_key = self
            .api_key
            .as_deref()
            .filter(|k| !k.trim().is_empty())
            .ok_or(TranscriptionError::MissingApiKey)?;

        let wav = audio.to_wav();
        drop(audio);
        log::debug!("uploading {} bytes of wav", wav.len());

        let mut form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(wav)
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| TranscriptionError::InvalidResponse(e.to_string()))?,
        );
        for (name, value) in self.config.request_fields() {
            form = form.text(name, value);
        }

        let response = self
            .client
            .post(self.config.endpoint())
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(map_reqwest_error)?;
        parse_response(status, &body)
    }
}

fn map_reqwest_error(err: reqwest::Error) -> TranscriptionError {
    if err.is_timeout() {
        TranscriptionError::Timeout
    } else {
        // `without_url` so a misconfigured base URL can never leak a key
        // that someone put into the URL.
        TranscriptionError::Network(err.without_url().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_fields_carry_model_and_json_format_and_optional_language() {
        let cfg = OpenAiConfig::default();
        assert_eq!(
            cfg.request_fields(),
            vec![
                ("model", DEFAULT_MODEL.to_owned()),
                ("response_format", "json".to_owned())
            ]
        );

        let cfg = OpenAiConfig {
            language: Some("ja".into()),
            model: "whisper-1".into(),
            ..Default::default()
        };
        assert_eq!(
            cfg.request_fields(),
            vec![
                ("model", "whisper-1".to_owned()),
                ("response_format", "json".to_owned()),
                ("language", "ja".to_owned()),
            ]
        );
    }

    #[test]
    fn endpoint_tolerates_trailing_slash() {
        let cfg = OpenAiConfig {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        assert_eq!(
            cfg.endpoint(),
            "https://example.test/v1/audio/transcriptions"
        );
    }

    #[test]
    fn success_body_yields_text() {
        assert_eq!(
            parse_response(200, r#"{"text":"こんにちは"}"#),
            Ok("こんにちは".into())
        );
    }

    #[test]
    fn malformed_success_body_is_invalid_response() {
        assert!(matches!(
            parse_response(200, "not json"),
            Err(TranscriptionError::InvalidResponse(_))
        ));
        assert!(matches!(
            parse_response(200, r#"{"nope":1}"#),
            Err(TranscriptionError::InvalidResponse(_))
        ));
    }

    #[test]
    fn auth_and_rate_limit_statuses_map_to_dedicated_errors() {
        assert_eq!(
            parse_response(401, "{}"),
            Err(TranscriptionError::Unauthorized)
        );
        assert_eq!(
            parse_response(429, "{}"),
            Err(TranscriptionError::RateLimited)
        );
    }

    #[test]
    fn other_errors_carry_status_and_api_message_without_the_body_dump() {
        assert_eq!(
            parse_response(500, r#"{"error":{"message":"server exploded","type":"x"}}"#),
            Err(TranscriptionError::Api {
                status: 500,
                message: "server exploded".into()
            })
        );
        assert_eq!(
            parse_response(502, "<html>bad gateway</html>"),
            Err(TranscriptionError::Api {
                status: 502,
                message: "unexpected response".into()
            })
        );
    }

    #[tokio::test]
    async fn missing_api_key_fails_before_any_network_call() {
        let provider = OpenAiTranscriptionProvider::new(
            None,
            OpenAiConfig {
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
