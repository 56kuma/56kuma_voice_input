//! Speech-to-text providers behind one vendor-neutral trait.

pub mod dictionary;
pub mod elevenlabs;
pub mod fillers;
pub mod groq;
pub mod openai;
pub mod provider;
pub mod xai;

pub use provider::{TranscriptionError, TranscriptionProvider};
