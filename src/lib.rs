//! voice_input — a lightweight background voice-input tool.
//!
//! Architecture rule: `Business Logic != OS API != UI != Network`.
//! Everything that talks to the OS, the network, or the screen lives behind a
//! trait and is kept as thin as possible (Humble Object). The decision-making
//! lives in pure, testable modules under `app/`.

pub mod app;
pub mod audio;
pub mod doubles;
pub mod hotkey;
pub mod input;
pub mod transcription;
pub mod ui;
