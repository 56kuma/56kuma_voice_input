//! Wayland: compositors forbid synthetic input from ordinary clients, so
//! this delegates to a helper if one is installed:
//!   1. `wtype`   (wlroots compositors: sway, hyprland, ...; types Unicode)
//!   2. `ydotool` (any compositor incl. GNOME; needs the ydotoold daemon and
//!      /dev/uinput access). `ydotool type` cannot type non-ASCII, so the
//!      text goes through the clipboard and ydotool only presses Ctrl+V.
//!
//! Otherwise it leaves the text in the clipboard and reports Unsupported so
//! the user sees the error state and can paste manually.

use std::process::{Command, Stdio};
use std::time::Duration;

use super::clipboard::{copy_only, paste_via_clipboard_settle, PasteKeySender};
use super::{InputError, TextInjector};

/// `ydotool key` arguments for Ctrl+V as Linux evdev codes
/// (KEY_LEFTCTRL = 29, KEY_V = 47; `code:1` press, `code:0` release).
pub const YDOTOOL_CTRL_V: [&str; 5] = ["key", "29:1", "47:1", "47:0", "29:0"];

/// uinput events reach the app asynchronously; give it time to read the
/// clipboard before the previous content is restored.
/// Socket used when `YDOTOOL_SOCKET` is unset; matches
/// `scripts/setup-ydotool-ubuntu.sh` (defaults differ between versions).
pub const YDOTOOL_DEFAULT_SOCKET: &str = "/tmp/.ydotool_socket";

const YDOTOOL_PASTE_SETTLE: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct WaylandTextInjector;

fn run_helper(program: &str, args: &[&str]) -> Option<Result<(), InputError>> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Some(Ok(())),
        Ok(s) => Some(Err(InputError::Failed(format!(
            "{program} exited with {s}"
        )))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(InputError::Failed(format!("{program}: {e}")))),
    }
}

/// Presses Ctrl+V through ydotoold. Its stderr carries no user text (only
/// key codes are passed), so it is safe to surface for diagnosis.
struct YdotoolPaste;

impl PasteKeySender for YdotoolPaste {
    fn send_paste(&self) -> Result<(), InputError> {
        let mut command = Command::new("ydotool");
        if std::env::var_os("YDOTOOL_SOCKET").is_none() {
            command.env("YDOTOOL_SOCKET", YDOTOOL_DEFAULT_SOCKET);
        }
        let output = command.args(YDOTOOL_CTRL_V).stdin(Stdio::null()).output();
        match output {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(InputError::Failed(format!(
                "ydotool exited with {}: {} (is ydotoold running?)",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            ))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(InputError::Unsupported("ydotool is not installed".into()))
            }
            Err(e) => Err(InputError::Failed(format!("ydotool: {e}"))),
        }
    }
}

impl TextInjector for WaylandTextInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        if let Some(result) = run_helper("wtype", &["--", text]) {
            if result.is_ok() {
                return result;
            }
            log::warn!("wtype failed ({result:?}); trying ydotool");
        }
        match paste_via_clipboard_settle(text, &YdotoolPaste, YDOTOOL_PASTE_SETTLE) {
            Ok(()) => Ok(()),
            Err(e) => {
                // The failed paste restored the old clipboard; put the text
                // back so the user can still paste it by hand.
                copy_only(text)?;
                Err(InputError::Unsupported(format!(
                    "{e}; text copied to clipboard, press Ctrl+V"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ydotool_presses_and_releases_ctrl_v_in_order() {
        assert_eq!(YDOTOOL_CTRL_V, ["key", "29:1", "47:1", "47:0", "29:0"]);
    }
}
