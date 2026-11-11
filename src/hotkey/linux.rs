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

/// App ID announced to the portal. Recent xdg-desktop-portal (1.19+, e.g.
/// Ubuntu 26.04) refuses `GlobalShortcuts` from an unsandboxed app with
/// "An app id is required" unless it first registers one. Must be a valid
/// D-Bus name (no element may start with a digit).
pub const APP_ID: &str = "io.github.kuma56.VoiceInput";

/// The `.desktop` entry the portal looks up for `APP_ID` ("App info not
/// found" otherwise). Hidden from the app menu; `exec` is this binary.
pub(crate) fn desktop_entry(exec: &std::path::Path) -> String {
    let mut quoted = String::new();
    for c in exec.to_string_lossy().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=voice_input\n\
         Comment=Press a hotkey, speak, and the text is typed\n\
         Exec=\"{quoted}\"\n\
         Terminal=false\n\
         NoDisplay=true\n"
    )
}

/// Writes `~/.local/share/applications/<APP_ID>.desktop` unless it is
/// already up to date. Returns whether the file changed.
fn ensure_desktop_entry() -> std::io::Result<bool> {
    let exe = std::env::current_exe()?;
    let dir = directories::BaseDirs::new()
        .ok_or_else(|| std::io::Error::other("no home directory"))?
        .data_dir()
        .join("applications");
    let path = dir.join(format!("{APP_ID}.desktop"));
    let content = desktop_entry(&exe);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, content)?;
    log::info!("installed {}", path.display());
    Ok(true)
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

    // Must precede every other portal call on this D-Bus connection. Older
    // portals lack the Registry interface; they don't need it, so go on.
    // The portal resolves the app id through its .desktop file; a freshly
    // written one needs a moment to be noticed by the portal's file monitor.
    match ensure_desktop_entry() {
        Ok(true) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        Ok(false) => {}
        Err(e) => log::warn!("could not install the .desktop entry: {e}"),
    }
    match ashpd::AppID::try_from(APP_ID) {
        Ok(app_id) => {
            if let Err(e) = ashpd::register_host_app(app_id).await {
                log::warn!("portal app id registration failed (continuing): {e}");
            }
        }
        Err(e) => log::warn!("invalid portal app id {APP_ID:?}: {e}"),
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_app_id_is_a_valid_dbus_app_id() {
        assert!(ashpd::AppID::try_from(APP_ID).is_ok());
        // The repo name's leading digit would be rejected.
        assert!(ashpd::AppID::try_from("io.github.56kuma.VoiceInput").is_err());
    }

    #[test]
    fn desktop_entry_points_at_the_binary_and_escapes_the_exec_path() {
        let entry = desktop_entry(std::path::Path::new("/opt/voice input/voice_input"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Type=Application\n"));
        assert!(entry.contains("Exec=\"/opt/voice input/voice_input\"\n"));
        assert!(!entry.contains("Hidden=true")); // Hidden means "deleted"

        let entry = desktop_entry(std::path::Path::new("/a$b/vi"));
        assert!(entry.contains(r#"Exec="/a\$b/vi""#));
    }
}
