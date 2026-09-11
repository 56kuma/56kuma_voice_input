//! Text injection into the currently focused application.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("text injection is not supported on this display server: {0}")]
    Unsupported(String),
    #[error("text injection failed: {0}")]
    Failed(String),
}

/// Types text at the current cursor position of whatever app has focus.
pub trait TextInjector: Send {
    fn insert(&self, text: &str) -> Result<(), InputError>;
}
