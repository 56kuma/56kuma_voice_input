//! Top-bar status icon (StatusNotifierItem via `ksni`). Linux only.
//!
//! Why this exists: on Wayland a normal window cannot position itself or
//! stay on top, so the capsule gets buried under other windows. The panel
//! is always visible and never covers work, so the state lives there too.
//!
//! Humble Object: `tray_icon` draws, `StateView` decides; this file only
//! forwards. `render` is called from the controller's tokio thread and
//! ksni's blocking API must not run inside a runtime, so states are handed
//! to a dedicated "tray" thread over a channel.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ksni::blocking::TrayMethods;

use super::state_view::{StateView, COMPLETE_FLASH};
use super::tray_icon;
use super::Overlay;
use crate::app::state::State;

/// Icon sizes offered to the panel (it picks the best fit; 64 covers HiDPI).
const SIZES: [u32; 3] = [22, 32, 64];

/// The `Overlay` that mirrors the state into the desktop panel.
pub struct TrayOverlay {
    states: Sender<State>,
}

impl TrayOverlay {
    /// Starts the tray in the background. Never fails: if the desktop has no
    /// tray host the reason is logged and `render` becomes a no-op.
    /// `on_quit` runs when the user picks "終了" from the icon's menu.
    pub fn spawn(on_quit: Box<dyn Fn() + Send>) -> Self {
        let (states, inbox) = mpsc::channel::<State>();
        let spawned = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || {
                let tray = Tray {
                    view: StateView::new(),
                    on_quit,
                };
                // "assume available": at login we may start before the
                // panel's tray host does; the icon appears once it is up.
                let handle = match tray.assume_sni_available(true).spawn() {
                    Ok(handle) => handle,
                    Err(e) => {
                        log::warn!("tray icon unavailable: {e}");
                        return;
                    }
                };
                log::info!("tray icon registered");

                // When the ✓ flash ends nothing else would trigger a redraw,
                // so wake up once to swap it back to the idle ring.
                let mut flash_ends: Option<Instant> = None;
                loop {
                    let received = match flash_ends {
                        Some(at) => {
                            inbox.recv_timeout(at.saturating_duration_since(Instant::now()))
                        }
                        None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    let flashing = match received {
                        Ok(state) => handle.update(|tray| {
                            let now = Instant::now();
                            tray.view.apply(state, now);
                            state == State::Idle && tray.view.needs_animation(now)
                        }),
                        Err(RecvTimeoutError::Timeout) => handle.update(|_| false),
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    flash_ends = match flashing {
                        Some(true) => Some(Instant::now() + COMPLETE_FLASH + FLASH_SLACK),
                        Some(false) => None,
                        None => break, // tray service shut down
                    };
                }
            });
        if let Err(e) = spawned {
            log::warn!("tray icon unavailable: {e}");
        }
        Self { states }
    }
}

/// Makes sure the wake-up lands after the flash deadline, not on it.
const FLASH_SLACK: Duration = Duration::from_millis(50);

impl Overlay for TrayOverlay {
    fn render(&self, state: State) {
        // The tray thread may be gone (no tray host); that is fine.
        let _ = self.states.send(state);
    }
}

struct Tray {
    view: StateView,
    on_quit: Box<dyn Fn() + Send>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "voice_input".into()
    }

    fn title(&self) -> String {
        format!(
            "voice_input: {}",
            tray_icon::label(self.view.glyph(Instant::now()))
        )
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let glyph = self.view.glyph(Instant::now());
        SIZES
            .iter()
            .map(|&size| ksni::Icon {
                width: size as i32,
                height: size as i32,
                data: tray_icon::render(glyph, size),
            })
            .collect()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.title(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "終了".into(),
                activate: Box::new(|tray: &mut Self| (tray.on_quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}
