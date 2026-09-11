use crate::hotkey::windows::WindowsHotkeyProvider;
use crate::hotkey::{HotkeyError, HotkeyProvider};
use crate::input::windows::WindowsTextInjector;
use crate::input::TextInjector;

pub fn hotkey_provider() -> Result<Box<dyn HotkeyProvider>, HotkeyError> {
    Ok(Box::new(WindowsHotkeyProvider::new()?))
}

pub fn text_injector() -> Box<dyn TextInjector> {
    Box::new(WindowsTextInjector)
}
