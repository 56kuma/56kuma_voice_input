//! Speech-to-text providers behind one vendor-neutral trait.

pub mod provider;

pub use provider::{TranscriptionError, TranscriptionProvider};
