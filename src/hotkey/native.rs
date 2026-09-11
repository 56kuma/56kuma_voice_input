//! Shared adapter over the `global-hotkey` crate (Windows + X11).
//!
//! On Windows the manager must be created on a thread that runs a win32
//! message loop (the main/UI thread); on X11 the crate runs its own thread.

use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{Hotkey, HotkeyError, HotkeyProvider, Key, Modifier};

pub(crate) fn to_native(hotkey: &Hotkey) -> Result<HotKey, HotkeyError> {
    let mut mods = Modifiers::empty();
    for m in &hotkey.modifiers {
        mods |= match m {
            Modifier::Ctrl => Modifiers::CONTROL,
            Modifier::Alt => Modifiers::ALT,
            Modifier::Shift => Modifiers::SHIFT,
            Modifier::Super => Modifiers::SUPER,
        };
    }
    let name = match hotkey.key {
        Key::Space => "Space".to_owned(),
        Key::Letter(c) => format!("Key{}", c.to_ascii_uppercase()),
        Key::Function(n) => format!("F{n}"),
    };
    let code = Code::from_str(&name).map_err(|_| HotkeyError::Parse(hotkey.to_string()))?;
    Ok(HotKey::new(Some(mods), code))
}

pub struct NativeHotkeyProvider {
    manager: Option<GlobalHotKeyManager>,
}

impl NativeHotkeyProvider {
    pub fn new() -> Result<Self, HotkeyError> {
        let manager =
            GlobalHotKeyManager::new().map_err(|e| HotkeyError::Unsupported(e.to_string()))?;
        Ok(Self {
            manager: Some(manager),
        })
    }
}

impl HotkeyProvider for NativeHotkeyProvider {
    fn register(
        &mut self,
        hotkey: &Hotkey,
        on_toggle: Box<dyn Fn() + Send + Sync>,
    ) -> Result<(), HotkeyError> {
        let native = to_native(hotkey)?;
        let manager = self
            .manager
            .as_ref()
            .ok_or_else(|| HotkeyError::Unsupported("manager gone".into()))?;
        manager
            .register(native)
            .map_err(|e| HotkeyError::Register(hotkey.to_string(), e.to_string()))?;
        let id = native.id();
        let receiver = GlobalHotKeyEvent::receiver().clone();
        std::thread::Builder::new()
            .name("hotkey-listener".into())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    log::debug!("hotkey event {:?}", event);
                    if event.id() == id && event.state() == HotKeyState::Pressed {
                        on_toggle();
                    }
                }
                log::warn!("hotkey channel closed; hotkey no longer active");
            })
            .map_err(|e| HotkeyError::Register(hotkey.to_string(), e.to_string()))?;
        log::info!("registered global hotkey {hotkey}");
        Ok(())
    }
}
