//! Configuration file and credential resolution.

#[allow(clippy::module_inception)] // file layout mirrors the spec: config/config.rs
pub mod config;
pub mod credentials;

pub use config::AppConfig;
