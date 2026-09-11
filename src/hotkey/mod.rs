//! Global hotkey. The trait and the pure helpers (parsing, debouncing) live
//! here; OS adapters live in `windows.rs` / `linux.rs`.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(any(windows, target_os = "linux"))]
mod native;
#[cfg(windows)]
pub mod windows;

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("unrecognised hotkey {0:?}")]
    Parse(String),
    #[error("could not register global hotkey {0}: {1}")]
    Register(String, String),
    #[error("global hotkeys are not supported here: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Super,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Space,
    /// A single letter, stored upper-case.
    Letter(char),
    /// Function key F1..F24.
    Function(u8),
}

/// A modifier combination plus one key, e.g. `Ctrl+Space`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub modifiers: Vec<Modifier>,
    pub key: Key,
}

impl Hotkey {
    /// The MVP default. Never changes.
    pub fn default_toggle() -> Self {
        Self {
            modifiers: vec![Modifier::Ctrl],
            key: Key::Space,
        }
    }

    /// Parses `"Ctrl+Space"`, `"ctrl+alt+r"`, `"Super+F9"` (case-insensitive).
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let reject = || HotkeyError::Parse(text.to_owned());
        let mut modifiers = Vec::new();
        let mut key = None;
        for token in text.split('+').map(|t| t.trim().to_ascii_lowercase()) {
            if key.is_some() {
                // A key was already given; nothing may follow it.
                return Err(reject());
            }
            match token.as_str() {
                "ctrl" | "control" => modifiers.push(Modifier::Ctrl),
                "alt" | "option" => modifiers.push(Modifier::Alt),
                "shift" => modifiers.push(Modifier::Shift),
                "super" | "win" | "cmd" | "meta" => modifiers.push(Modifier::Super),
                "space" => key = Some(Key::Space),
                t if t.len() == 1 && t.chars().all(|c| c.is_ascii_alphabetic()) => {
                    key = Some(Key::Letter(t.chars().next().unwrap().to_ascii_uppercase()));
                }
                t => match t.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                    Some(n) if (1..=24).contains(&n) => key = Some(Key::Function(n)),
                    _ => return Err(reject()),
                },
            }
        }
        Ok(Self {
            modifiers,
            key: key.ok_or_else(reject)?,
        })
    }
}

impl std::fmt::Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for m in &self.modifiers {
            write!(f, "{m:?}+")?;
        }
        match self.key {
            Key::Space => write!(f, "Space"),
            Key::Letter(c) => write!(f, "{c}"),
            Key::Function(n) => write!(f, "F{n}"),
        }
    }
}

/// Registers a toggle hotkey and calls `on_toggle` each time it fires.
///
/// The registration lives exactly as long as the provider: dropping it
/// unregisters the hotkey, so the caller must keep it alive.
pub trait HotkeyProvider {
    fn register(
        &mut self,
        hotkey: &Hotkey,
        on_toggle: Box<dyn Fn() + Send + Sync>,
    ) -> Result<(), HotkeyError>;
}

/// Drops repeated presses that arrive within `window` of the last accepted
/// press (key auto-repeat, bouncing keyboards, duplicate OS events).
#[derive(Debug)]
pub struct Debouncer {
    window: Duration,
    last_accepted: Option<Instant>,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_accepted: None,
        }
    }

    /// Returns `true` if the press at `now` should be acted upon.
    pub fn accept(&mut self, now: Instant) -> bool {
        let too_soon = self
            .last_accepted
            .is_some_and(|last| now.duration_since(last) < self.window);
        if too_soon {
            return false;
        }
        self.last_accepted = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey_is_ctrl_space() {
        assert_eq!(
            Hotkey::default_toggle(),
            Hotkey {
                modifiers: vec![Modifier::Ctrl],
                key: Key::Space
            }
        );
        assert_eq!(Hotkey::default_toggle().to_string(), "Ctrl+Space");
    }

    #[test]
    fn parses_ctrl_space_case_insensitively_with_spaces() {
        for text in [
            "Ctrl+Space",
            "ctrl+space",
            " CTRL + SPACE ",
            "Control+Space",
        ] {
            assert_eq!(
                Hotkey::parse(text).unwrap(),
                Hotkey::default_toggle(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn parses_multiple_modifiers_letters_and_function_keys() {
        assert_eq!(
            Hotkey::parse("ctrl+alt+r").unwrap(),
            Hotkey {
                modifiers: vec![Modifier::Ctrl, Modifier::Alt],
                key: Key::Letter('R')
            }
        );
        assert_eq!(
            Hotkey::parse("Super+Shift+F9").unwrap(),
            Hotkey {
                modifiers: vec![Modifier::Super, Modifier::Shift],
                key: Key::Function(9)
            }
        );
    }

    #[test]
    fn rejects_hotkeys_without_a_key_or_with_unknown_tokens() {
        for text in ["", "Ctrl", "Ctrl+", "Ctrl+Bogus", "Space+Ctrl+Space", "F99"] {
            assert!(
                matches!(Hotkey::parse(text), Err(HotkeyError::Parse(_))),
                "{text:?} should be rejected"
            );
        }
    }

    #[test]
    fn debouncer_accepts_first_press_and_rejects_immediate_repeat() {
        let mut d = Debouncer::new(Duration::from_millis(250));
        let t0 = Instant::now();

        assert!(d.accept(t0));
        assert!(!d.accept(t0 + Duration::from_millis(100)));
        assert!(!d.accept(t0 + Duration::from_millis(249)));
    }

    #[test]
    fn debouncer_accepts_press_after_window_measured_from_last_accepted() {
        let mut d = Debouncer::new(Duration::from_millis(250));
        let t0 = Instant::now();
        d.accept(t0);
        d.accept(t0 + Duration::from_millis(200)); // rejected, must not extend the window

        assert!(d.accept(t0 + Duration::from_millis(250)));
    }
}
