//! Chooses the OS adapters. The detection logic is pure and tested; the
//! constructors are thin.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

use crate::hotkey::{HotkeyError, HotkeyProvider};
use crate::input::TextInjector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
    Unknown,
}

/// Decides Wayland vs X11 from the session environment.
/// `XDG_SESSION_TYPE` wins; then `WAYLAND_DISPLAY`; then `DISPLAY`.
pub fn detect_display_server(env: impl Fn(&str) -> Option<String>) -> DisplayServer {
    let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    match env("XDG_SESSION_TYPE")
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("wayland") => DisplayServer::Wayland,
        Some("x11") => DisplayServer::X11,
        _ if set("WAYLAND_DISPLAY") => DisplayServer::Wayland,
        _ if set("DISPLAY") => DisplayServer::X11,
        _ => DisplayServer::Unknown,
    }
}

/// The platform's hotkey provider, or why none is available.
pub fn hotkey_provider() -> Result<Box<dyn HotkeyProvider>, HotkeyError> {
    #[cfg(windows)]
    {
        windows::hotkey_provider()
    }
    #[cfg(target_os = "linux")]
    {
        linux::hotkey_provider()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Err(HotkeyError::Unsupported("unsupported platform".into()))
    }
}

pub fn text_injector() -> Box<dyn TextInjector> {
    #[cfg(windows)]
    {
        windows::text_injector()
    }
    #[cfg(target_os = "linux")]
    {
        linux::text_injector()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Box::new(crate::input::clipboard::CopyOnlyInjector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn xdg_session_type_is_authoritative() {
        assert_eq!(
            detect_display_server(env(&[("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":0")])),
            DisplayServer::Wayland
        );
        assert_eq!(
            detect_display_server(env(&[
                ("XDG_SESSION_TYPE", "x11"),
                ("WAYLAND_DISPLAY", "wayland-0")
            ])),
            DisplayServer::X11
        );
    }

    #[test]
    fn wayland_display_beats_display_when_session_type_is_missing() {
        assert_eq!(
            detect_display_server(env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")])),
            DisplayServer::Wayland
        );
        assert_eq!(
            detect_display_server(env(&[("DISPLAY", ":0")])),
            DisplayServer::X11
        );
    }

    #[test]
    fn nothing_set_is_unknown() {
        assert_eq!(detect_display_server(env(&[])), DisplayServer::Unknown);
        assert_eq!(
            detect_display_server(env(&[("XDG_SESSION_TYPE", "tty")])),
            DisplayServer::Unknown
        );
    }
}
