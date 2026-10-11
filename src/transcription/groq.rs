//! Groq speech-to-text. Groq serves an OpenAI-compatible
//! `/audio/transcriptions`, so it runs on `OpenAiTranscriptionProvider`
//! and only the defaults differ.

pub const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";
pub const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";
