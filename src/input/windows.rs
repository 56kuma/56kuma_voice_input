//! Windows: `SendInput` with `KEYEVENTF_UNICODE`, one key-down/up pair per
//! UTF-16 unit. Works in Win32, UWP, Electron, and Windows Terminal.
//! Falls back to clipboard + Ctrl+V if SendInput rejects the batch.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_RETURN, VK_V,
};

use super::clipboard::{paste_via_clipboard, PasteKeySender};
use super::{InputError, TextInjector};

#[derive(Default)]
pub struct WindowsTextInjector;

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), InputError> {
    if inputs.is_empty() {
        return Ok(());
    }
    // SAFETY: `inputs` is a valid slice of fully-initialised INPUT structs.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InputError::Failed(format!(
            "SendInput sent {sent}/{}",
            inputs.len()
        )))
    }
}

fn unicode_inputs(text: &str) -> Vec<INPUT> {
    let mut inputs = Vec::with_capacity(text.len() * 2);
    for line in text.split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(body) => (body.strip_suffix('\r').unwrap_or(body), true),
            None => (line, false),
        };
        for unit in body.encode_utf16() {
            inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
            inputs.push(key(
                VIRTUAL_KEY(0),
                unit,
                KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
            ));
        }
        if newline {
            inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
        }
    }
    inputs
}

impl PasteKeySender for WindowsTextInjector {
    fn send_paste(&self) -> Result<(), InputError> {
        send(&[
            key(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
            key(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
            key(VK_V, 0, KEYEVENTF_KEYUP),
            key(VK_CONTROL, 0, KEYEVENTF_KEYUP),
        ])
    }
}

impl TextInjector for WindowsTextInjector {
    fn insert(&self, text: &str) -> Result<(), InputError> {
        match send(&unicode_inputs(text)) {
            Ok(()) => Ok(()),
            Err(e) => {
                log::warn!("SendInput failed ({e}); falling back to clipboard paste");
                paste_via_clipboard(text, self)
            }
        }
    }
}
