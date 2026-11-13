//! Floating status overlay. The trait is the seam; `overlay` is the eframe
//! adapter and `state_view` is the pure mapping from State to visuals.

pub mod overlay;
pub mod state_view;
#[cfg(target_os = "linux")]
pub mod tray;
pub mod tray_icon;

use crate::app::state::State;

/// Anything that can show the current state to the user.
pub trait Overlay: Send {
    fn render(&self, state: State);
}

/// Shows the state in several places at once (capsule + top-bar icon).
pub struct Fanout(pub Vec<Box<dyn Overlay>>);

impl Overlay for Fanout {
    fn render(&self, state: State) {
        for overlay in &self.0 {
            overlay.render(state);
        }
    }
}
