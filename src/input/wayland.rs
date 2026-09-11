//! Wayland: compositors forbid synthetic input from ordinary clients, so the
//! MVP delegates to a helper if one is installed:
//!   1. `wtype`   (wlroots compositors: sway, hyprland, ...)
//!   2. `ydotool` (any compositor; needs the ydotoold daemon / uinput)
//!
//! Otherwise it leaves the text in the clipboard and reports Unsupported so
//! the user sees the error state and can paste manually.
//!
//! Future: XDG RemoteDesktop portal or an IBus input-method engine can be
//! added here without touching the rest of the app.

use std::process::{Command, Stdio};

use super::clipboard::copy_only;
use super::{InputError, TextInjector};

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

impl TextInjector for WaylandTextInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        if let Some(result) = run_helper("wtype", &["--", text]) {
            if result.is_ok() {
                return result;
            }
            log::warn!("wtype failed ({result:?}); trying ydotool");
        }
        if let Some(result) = run_helper("ydotool", &["type", "--", text]) {
            return result;
        }
        copy_only(text)?;
        Err(InputError::Unsupported(
            "Wayland without wtype/ydotool: text copied to clipboard, press Ctrl+V".into(),
        ))
    }
}
