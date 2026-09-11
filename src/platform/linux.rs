use super::{detect_display_server, DisplayServer};
use crate::hotkey::linux::{WaylandPortalHotkeyProvider, X11HotkeyProvider};
use crate::hotkey::{HotkeyError, HotkeyProvider};
use crate::input::wayland::WaylandTextInjector;
use crate::input::x11::X11TextInjector;
use crate::input::TextInjector;

fn display_server() -> DisplayServer {
    let server = detect_display_server(|name| std::env::var(name).ok());
    log::info!("display server: {server:?}");
    server
}

pub fn hotkey_provider() -> Result<Box<dyn HotkeyProvider>, HotkeyError> {
    match display_server() {
        DisplayServer::Wayland => Ok(Box::new(WaylandPortalHotkeyProvider)),
        DisplayServer::X11 => Ok(Box::new(X11HotkeyProvider::new()?)),
        DisplayServer::Unknown => Err(HotkeyError::Unsupported(
            "neither WAYLAND_DISPLAY nor DISPLAY is set".into(),
        )),
    }
}

pub fn text_injector() -> Box<dyn TextInjector> {
    match display_server() {
        DisplayServer::Wayland => Box::new(WaylandTextInjector),
        _ => Box::new(X11TextInjector),
    }
}
