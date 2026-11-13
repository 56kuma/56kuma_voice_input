//! ElevenLabs Scribe `/v1/speech-to-text` provider.
//!
//! Multipart like OpenAI, but the key goes in an `xi-api-key` header and the
//! fields are `model_id` / `language_code`. Audio-event tags such as
//! "(laughter)" are turned off: they would be typed into the user's text.
//! `no_verbatim` asks the model to leave out filler words and false starts.

use super::openai::{post_transcription, Auth};
use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::audio_data::AudioData;

pub const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io/v1";
pub const DEFAULT_MODEL: &str = "scribe_v3";
pub const AUTH_HEADER: &str = "xi-api-key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevenLabsConfig {
    pub base_url: String,
    pub model: String,
    /// ISO-639-1/3 code such as `"ja"`. `None` lets the API auto-detect.
    pub language: Option<String>,
    pub timeout_secs: u64,
    /// Sent as `no_verbatim`: drop filler words, false starts and
    /// non-speech sounds (scribe_v2 and later).
    pub no_verbatim: bool,
}

impl Default for ElevenLabsConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            language: None,
            timeout_secs: 30,
            no_verbatim: true,
        }
    }
}

impl ElevenLabsConfig {
    /// The non-file multipart fields for one request.
    pub fn request_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("model_id", self.model.clone()),
            ("tag_audio_events", "false".to_owned()),
        ];
        if self.no_verbatim {
            fields.push(("no_verbatim", "true".to_owned()));
        }
        if let Some(language) = &self.language {
            fields.push(("language_code", language.clone()));
        }
        fields
    }

    pub fn endpoint(&self) -> String {
        format!("{}/speech-to-text", self.base_url.trim_end_matches('/'))
    }
}

pub struct ElevenLabsTranscriptionProvider {
    api_key: Option<String>,
    config: ElevenLabsConfig,
    client: reqwest::Client,
}

impl ElevenLabsTranscriptionProvider {
    pub fn new(api_key: Option<String>, config: ElevenLabsConfig) -> Self {
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
impl TranscriptionProvider for ElevenLabsTranscriptionProvider {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        post_transcription(
            &self.client,
            &self.config.endpoint(),
            self.api_key.as_deref(),
            Auth::Header(AUTH_HEADER),
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
    fn request_fields_carry_model_id_no_event_tags_and_optional_language_code() {
        assert_eq!(
            ElevenLabsConfig::default().request_fields(),
            vec![
                ("model_id", "scribe_v3".to_owned()),
                ("tag_audio_events", "false".to_owned()),
                ("no_verbatim", "true".to_owned()),
            ]
        );

        let cfg = ElevenLabsConfig {
            language: Some("ja".into()),
            no_verbatim: false,
            ..Default::default()
        };
        assert_eq!(
            cfg.request_fields(),
            vec![
                ("model_id", "scribe_v3".to_owned()),
                ("tag_audio_events", "false".to_owned()),
                ("language_code", "ja".to_owned()),
            ]
        );
    }

    #[test]
    fn endpoint_is_speech_to_text_and_tolerates_trailing_slash() {
        assert_eq!(
            ElevenLabsConfig::default().endpoint(),
            "https://api.elevenlabs.io/v1/speech-to-text"
        );
        let cfg = ElevenLabsConfig {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        assert_eq!(cfg.endpoint(), "https://example.test/v1/speech-to-text");
    }

    #[tokio::test]
    async fn the_key_is_sent_in_the_xi_api_key_header_not_as_a_bearer_token() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            while !request.ends_with(b"--\r\n") {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let body = r#"{"language_code":"jpn","text":"こんにちは","words":[]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });

        let provider = ElevenLabsTranscriptionProvider::new(
            Some("test-key".into()),
            ElevenLabsConfig {
                base_url: format!("http://{addr}/v1"),
                language: Some("ja".into()),
                ..Default::default()
            },
        );
        let result = provider
            .transcribe(AudioData::new(16_000, vec![0; 160]))
            .await;

        let request = server.join().unwrap();
        assert_eq!(result, Ok("こんにちは".into()));
        assert!(request.starts_with("POST /v1/speech-to-text "), "{request}");
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("xi-api-key: test-key\r\n"));
        assert!(!lower.contains("authorization:"));
        let file = request.find(r#"name="file""#).unwrap();
        assert!(request.find(r#"name="model_id""#).unwrap() < file);
        assert!(request.find(r#"name="language_code""#).unwrap() < file);
    }

    #[tokio::test]
    async fn missing_api_key_fails_before_any_network_call() {
        let provider = ElevenLabsTranscriptionProvider::new(
            None,
            ElevenLabsConfig {
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
