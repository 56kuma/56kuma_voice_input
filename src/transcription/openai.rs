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

    /// `request_fields` plus the hints as the `prompt` field, which both
    /// OpenAI and Groq use to bias the spelling of the words listed in it.
    pub fn request_fields_with_hints(&self, hints: &[String]) -> Vec<(&'static str, String)> {
        let mut fields = self.request_fields();
        if !hints.is_empty() {
            fields.push(("prompt", hints.join(", ")));
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
            let message = error_message(body).unwrap_or_else(|| "unexpected response".to_owned());
            Err(TranscriptionError::Api { status, message })
        }
    }
}

#[derive(serde::Deserialize)]
struct SuccessBody {
    text: String,
}

/// `{"error": {"message": "..."}}` (OpenAI), `{"error": "..."}` (xAI), or
/// ElevenLabs' `{"detail": {"message": "..."}}` / `{"detail": "..."}` /
/// `{"detail": [{"msg": "..."}]}` (validation errors).
fn error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if let Some(error) = value.get("error") {
        return error
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| error.as_str())
            .map(str::to_owned);
    }
    let detail = value.get("detail")?;
    detail
        .get("message")
        .or_else(|| detail.get(0).and_then(|d| d.get("msg")))
        .and_then(|m| m.as_str())
        .or_else(|| detail.as_str())
        .map(str::to_owned)
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
        self.transcribe_with_hints(audio, &[]).await
    }

    async fn transcribe_with_hints(
        &self,
        audio: AudioData,
        hints: &[String],
    ) -> Result<String, TranscriptionError> {
        post_transcription(
            &self.client,
            &self.config.endpoint(),
            self.api_key.as_deref(),
            Auth::Bearer,
            self.config.request_fields_with_hints(hints),
            audio,
        )
        .await
    }
}

/// How the API key is attached to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Auth {
    /// `Authorization: Bearer <key>` (OpenAI, Groq, xAI).
    Bearer,
    /// `<name>: <key>` (ElevenLabs' `xi-api-key`).
    Header(&'static str),
}

/// The one HTTP call shared by every multipart speech-to-text API: option
/// fields first, the audio file LAST (xAI ignores fields that follow the
/// file; OpenAI accepts any order), JSON `{"text": ...}` back.
pub(crate) async fn post_transcription(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    auth: Auth,
    fields: Vec<(&'static str, String)>,
    audio: AudioData,
) -> Result<String, TranscriptionError> {
    let api_key = api_key
        .filter(|k| !k.trim().is_empty())
        .ok_or(TranscriptionError::MissingApiKey)?;

    let wav = audio.to_wav();
    drop(audio);
    log::debug!("uploading {} bytes of wav", wav.len());

    let mut form = reqwest::multipart::Form::new();
    for (name, value) in fields {
        form = form.text(name, value);
    }
    let form = form.part(
        "file",
        reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscriptionError::InvalidResponse(e.to_string()))?,
    );

    let request = match auth {
        Auth::Bearer => client.post(endpoint).bearer_auth(api_key),
        Auth::Header(name) => {
            // A key with characters a header cannot carry is a bad key.
            let mut value = reqwest::header::HeaderValue::from_str(api_key)
                .map_err(|_| TranscriptionError::Unauthorized)?;
            value.set_sensitive(true); // like bearer_auth: never in Debug output
            client.post(endpoint).header(name, value)
        }
    };
    let response = request
        .multipart(form)
        .send()
        .await
        .map_err(map_reqwest_error)?;

    let status = response.status().as_u16();
    let body = response.text().await.map_err(map_reqwest_error)?;
    parse_response(status, &body)
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
    fn hints_are_sent_as_the_prompt_and_omitted_when_there_are_none() {
        let cfg = OpenAiConfig::default();
        assert_eq!(cfg.request_fields_with_hints(&[]), cfg.request_fields());
        let hints = ["Claude Code".to_owned(), "GitHub".to_owned()];
        assert_eq!(
            cfg.request_fields_with_hints(&hints).last(),
            Some(&("prompt", "Claude Code, GitHub".to_owned()))
        );
    }

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

    #[test]
    fn a_plain_string_error_field_is_used_as_the_message() {
        assert_eq!(
            parse_response(400, r#"{"code":"bad","error":"unsupported audio"}"#),
            Err(TranscriptionError::Api {
                status: 400,
                message: "unsupported audio".into()
            })
        );
    }

    #[test]
    fn elevenlabs_detail_errors_are_used_as_the_message() {
        assert_eq!(
            parse_response(
                400,
                r#"{"detail":{"status":"invalid_model","message":"model not found"}}"#
            ),
            Err(TranscriptionError::Api {
                status: 400,
                message: "model not found".into()
            })
        );
        assert_eq!(
            parse_response(
                422,
                r#"{"detail":[{"loc":["body","file"],"msg":"field required","type":"missing"}]}"#
            ),
            Err(TranscriptionError::Api {
                status: 422,
                message: "field required".into()
            })
        );
        assert_eq!(
            parse_response(400, r#"{"detail":"bad audio"}"#),
            Err(TranscriptionError::Api {
                status: 400,
                message: "bad audio".into()
            })
        );
    }

    #[tokio::test]
    async fn the_audio_file_is_sent_after_every_option_field() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            // The multipart body ends with the closing boundary "--\r\n".
            while !request.ends_with(b"--\r\n") {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let body = r#"{"text":"ok"}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });

        let result = post_transcription(
            &reqwest::Client::new(),
            &format!("http://{addr}/stt"),
            Some("test-key"),
            Auth::Bearer,
            vec![("model", "m".into()), ("language", "ja".into())],
            AudioData::new(16_000, vec![0; 160]),
        )
        .await;

        let request = server.join().unwrap();
        assert_eq!(result, Ok("ok".into()));
        let file = request.find(r#"name="file""#).unwrap();
        assert!(request.find(r#"name="model""#).unwrap() < file);
        assert!(request.find(r#"name="language""#).unwrap() < file);
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
