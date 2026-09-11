//! Clipboard-assisted insertion shared by every platform:
//! save clipboard → set text → paste shortcut → restore clipboard.

use std::time::Duration;

use super::InputError;

/// Sends the paste chord (Ctrl+V) to the focused window.
pub trait PasteKeySender {
    fn send_paste(&self) -> Result<(), InputError>;
}

/// Time to let the target application read the clipboard before restoring.
const PASTE_SETTLE: Duration = Duration::from_millis(120);

pub fn paste_via_clipboard(text: &str, keys: &dyn PasteKeySender) -> Result<(), InputError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| InputError::Failed(e.to_string()))?;
    let previous = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_owned())
        .map_err(|e| InputError::Failed(e.to_string()))?;
    let result = keys.send_paste();
    std::thread::sleep(PASTE_SETTLE);
    match previous {
        Some(old) => {
            if let Err(e) = clipboard.set_text(old) {
                log::warn!("could not restore clipboard: {e}");
            }
        }
        None => {
            let _ = clipboard.clear();
        }
    }
    result
}

/// Leaves the text in the clipboard so the user can paste it manually.
pub fn copy_only(text: &str) -> Result<(), InputError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| InputError::Failed(e.to_string()))?;
    clipboard
        .set_text(text.to_owned())
        .map_err(|e| InputError::Failed(e.to_string()))
}

/// Last-resort injector for platforms without a native adapter.
pub struct CopyOnlyInjector;

impl super::TextInjector for CopyOnlyInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        copy_only(text)?;
        Err(InputError::Unsupported(
            "no native text injection on this platform; text copied to clipboard".into(),
        ))
    }
}
