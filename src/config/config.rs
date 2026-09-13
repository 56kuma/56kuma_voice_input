//! `config.toml` — every field is optional and has a sensible default.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    /// Toggle hotkey, e.g. `"Ctrl+Space"`. The MVP default is fixed.
    pub hotkey: String,
    /// Which `TranscriptionProvider` to use. MVP: `"openai"` only.
    pub provider: String,
    /// Seconds the Error state stays visible before returning to Idle.
    pub error_display_secs: u64,
    pub audio: AudioConfig,
    pub openai: OpenAiSection,
    pub overlay: OverlayConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    /// Sample rate sent to the API. 16 kHz mono is what speech models expect.
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenAiSection {
    pub model: String,
    pub language: Option<String>,
    pub base_url: String,
    pub timeout_secs: u64,
    /// Lowest-priority key source. Prefer the OS credential store.
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OverlayConfig {
    /// `"top-center"` (default), `"top-left"`, `"top-right"`.
    pub position: String,
    /// Distance from the screen edge in logical pixels.
    pub margin: f32,
    /// Overlay height in logical pixels; width follows the silver ratio.
    pub height: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid config: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Space".to_owned(),
            provider: "openai".to_owned(),
            error_display_secs: 3,
            audio: AudioConfig::default(),
            openai: OpenAiSection::default(),
            overlay: OverlayConfig::default(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
        }
    }
}

impl Default for OpenAiSection {
    fn default() -> Self {
        Self {
            model: crate::transcription::openai::DEFAULT_MODEL.to_owned(),
            language: None,
            base_url: crate::transcription::openai::DEFAULT_BASE_URL.to_owned(),
            timeout_secs: 30,
            api_key: None,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            position: "top-center".to_owned(),
            margin: 12.0,
            height: 44.0,
        }
    }
}

impl AppConfig {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }

    /// `<config dir>/voice_input/config.toml` for the current platform.
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "voice_input")
            .map(|d| d.config_dir().join("config.toml"))
    }

    /// Where `load()` looks, in priority order: `config.toml` in the
    /// working directory (repo-local override), then `default_path()`.
    pub fn candidate_paths() -> Vec<PathBuf> {
        let mut paths = vec![PathBuf::from("config.toml")];
        paths.extend(Self::default_path());
        paths
    }

    /// Loads the first candidate that exists; none existing means defaults.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_first(&Self::candidate_paths())
    }

    fn load_first(paths: &[PathBuf]) -> Result<Self, ConfigError> {
        for path in paths {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    log::info!("loaded config from {}", path.display());
                    return Self::from_toml(&text);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            }
        }
        log::info!("no config file found, using defaults");
        Ok(Self::default())
    }

    pub fn openai_config(&self) -> crate::transcription::openai::OpenAiConfig {
        crate::transcription::openai::OpenAiConfig {
            base_url: self.openai.base_url.clone(),
            model: self.openai.model.clone(),
            language: self.openai.language.clone(),
            timeout_secs: self.openai.timeout_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey_is_ctrl_space_and_provider_is_openai() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.hotkey, "Ctrl+Space");
        assert_eq!(cfg.provider, "openai");
        assert_eq!(cfg.audio.sample_rate, 16_000);
        assert!(cfg.openai.api_key.is_none());
    }

    #[test]
    fn empty_toml_equals_defaults() {
        assert_eq!(AppConfig::from_toml("").unwrap(), AppConfig::default());
    }

    #[test]
    fn partial_toml_overrides_only_the_given_fields() {
        let cfg = AppConfig::from_toml(
            r#"
            error_display_secs = 5
            [openai]
            model = "whisper-1"
            language = "ja"
            "#,
        )
        .unwrap();

        assert_eq!(cfg.error_display_secs, 5);
        assert_eq!(cfg.openai.model, "whisper-1");
        assert_eq!(cfg.openai.language.as_deref(), Some("ja"));
        assert_eq!(cfg.hotkey, "Ctrl+Space");
        assert_eq!(cfg.openai.base_url, AppConfig::default().openai.base_url);
    }

    #[test]
    fn unknown_fields_and_bad_syntax_are_errors_not_silently_ignored() {
        assert!(matches!(
            AppConfig::from_toml("hotkei = 'x'"),
            Err(ConfigError::Parse(_))
        ));
        assert!(matches!(
            AppConfig::from_toml("hotkey = "),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn candidate_paths_prefer_the_working_directory_config() {
        let paths = AppConfig::candidate_paths();
        assert_eq!(paths[0], PathBuf::from("config.toml"));
    }

    #[test]
    fn load_first_takes_the_first_existing_file_and_skips_missing_ones() {
        let dir = std::env::temp_dir().join(format!("voice_input_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("config.toml");
        std::fs::write(&existing, "error_display_secs = 7").unwrap();

        let cfg = AppConfig::load_first(&[dir.join("missing.toml"), existing]).unwrap();
        assert_eq!(cfg.error_display_secs, 7);

        let cfg = AppConfig::load_first(&[dir.join("missing.toml")]).unwrap();
        assert_eq!(cfg, AppConfig::default());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_path_ends_with_voice_input_config_toml() {
        let path = AppConfig::default_path().expect("a config dir exists on CI");
        // Linux: .../voice_input/config.toml; Windows: ...\voice_input\config\config.toml
        assert!(
            path.ends_with("voice_input/config.toml")
                || path.ends_with("voice_input/config/config.toml"),
            "{}",
            path.display()
        );
    }
}
