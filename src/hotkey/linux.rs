//! Linux: X11 uses `XGrabKey` via the `global-hotkey` crate; Wayland uses
//! the XDG desktop portal `GlobalShortcuts` interface (the only sanctioned
//! way to receive global shortcuts on GNOME/KDE Wayland).

pub use super::native::NativeHotkeyProvider as X11HotkeyProvider;

use super::{Hotkey, HotkeyError, HotkeyProvider, Key, Modifier};

/// Trigger string in the format the portal expects ("CTRL+space").
pub(crate) fn portal_trigger(hotkey: &Hotkey) -> String {
    let mut parts: Vec<String> = hotkey
        .modifiers
        .iter()
        .map(|m| {
            match m {
                Modifier::Ctrl => "CTRL",
                Modifier::Alt => "ALT",
                Modifier::Shift => "SHIFT",
                Modifier::Super => "LOGO",
            }
            .to_owned()
        })
        .collect();
    parts.push(match hotkey.key {
        Key::Space => "space".to_owned(),
        Key::Letter(c) => c.to_ascii_lowercase().to_string(),
        Key::Function(n) => format!("F{n}"),
    });
    parts.join("+")
}

pub struct WaylandPortalHotkeyProvider;

impl HotkeyProvider for WaylandPortalHotkeyProvider {
    fn register(
        &mut self,
        hotkey: &Hotkey,
        on_toggle: Box<dyn Fn() + Send + Sync>,
    ) -> Result<(), HotkeyError> {
        let trigger = portal_trigger(hotkey);
        let label = hotkey.to_string();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), HotkeyError>>();

        std::thread::Builder::new()
            .name("hotkey-portal".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(HotkeyError::Register(label, e.to_string())));
                        return;
                    }
                };
                runtime.block_on(portal_loop(trigger, label, ready_tx, on_toggle));
            })
            .map_err(|e| HotkeyError::Register(hotkey.to_string(), e.to_string()))?;

        ready_rx
            .recv()
            .unwrap_or_else(|_| Err(HotkeyError::Unsupported("portal thread died".into())))
    }
}

async fn portal_loop(
    trigger: String,
    label: String,
    ready: std::sync::mpsc::Sender<Result<(), HotkeyError>>,
    on_toggle: Box<dyn Fn() + Send + Sync>,
) {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    const ID: &str = "toggle-voice-input";

    let setup = async {
        let proxy = GlobalShortcuts::new().await?;
        let session = proxy.create_session().await?;
        let shortcut = NewShortcut::new(ID, "Toggle voice input (start / stop recording)")
            .preferred_trigger(Some(trigger.as_str()));
        let request = proxy.bind_shortcuts(&session, &[shortcut], None).await?;
        let bound = request.response()?;
        for s in bound.shortcuts() {
            log::info!(
                "portal bound shortcut {:?} as {:?}",
                s.id(),
                s.trigger_description()
            );
        }
        let stream = proxy.receive_activated().await?;
        Ok::<_, ashpd::Error>((proxy, session, stream))
    };

    let (_proxy, _session, mut stream) = match setup.await {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(HotkeyError::Register(label, e.to_string())));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    log::info!("registered global hotkey {label} via XDG portal");

    while let Some(activated) = stream.next().await {
        if activated.shortcut_id() == ID {
            on_toggle();
        }
    }
    log::warn!("portal shortcut stream ended; hotkey no longer active");
}
