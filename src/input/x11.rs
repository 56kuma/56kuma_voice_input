//! X11: prefers `xdotool type` (types real key events, works in terminals),
//! otherwise clipboard + Ctrl+V sent through the XTEST extension.

use std::process::{Command, Stdio};

use super::clipboard::{paste_via_clipboard, PasteKeySender};
use super::{InputError, TextInjector};

#[derive(Default)]
pub struct X11TextInjector;

fn xdotool_type(text: &str) -> Option<Result<(), InputError>> {
    let status = Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--delay", "2", "--", text])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Some(Ok(())),
        Ok(s) => Some(Err(InputError::Failed(format!("xdotool exited with {s}")))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(InputError::Failed(e.to_string()))),
    }
}

impl PasteKeySender for X11TextInjector {
    fn send_paste(&self) -> Result<(), InputError> {
        xtest::send_ctrl_v().map_err(InputError::Failed)
    }
}

impl TextInjector for X11TextInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        if let Some(result) = xdotool_type(text) {
            return result;
        }
        log::debug!("xdotool not found; using clipboard + XTEST Ctrl+V");
        paste_via_clipboard(text, self)
    }
}

mod xtest {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, Keycode};
    use x11rb::protocol::xtest::ConnectionExt as _;

    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_V: u32 = 0x0076;
    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;

    fn keycode_for(conn: &impl Connection, keysym: u32) -> Result<Keycode, String> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let per = usize::from(mapping.keysyms_per_keycode);
        for (i, syms) in mapping.keysyms.chunks(per).enumerate() {
            if syms.contains(&keysym) {
                return Ok(min + i as u8);
            }
        }
        Err(format!("no keycode for keysym {keysym:#x}"))
    }

    pub fn send_ctrl_v() -> Result<(), String> {
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let ctrl = keycode_for(&conn, XK_CONTROL_L)?;
        let v = keycode_for(&conn, XK_V)?;
        for (kind, code) in [
            (KEY_PRESS, ctrl),
            (KEY_PRESS, v),
            (KEY_RELEASE, v),
            (KEY_RELEASE, ctrl),
        ] {
            conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, x11rb::NONE, 0, 0, 0)
                .map_err(|e| e.to_string())?;
        }
        conn.flush().map_err(|e| e.to_string())?;
        Ok(())
    }
}
