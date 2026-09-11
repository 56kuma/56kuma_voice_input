//! Floating status overlay. The trait is the seam; `overlay` is the eframe
//! adapter and `state_view` is the pure mapping from State to visuals.

pub mod overlay;
pub mod state_view;

use crate::app::state::State;

/// Anything that can show the current state to the user.
pub trait Overlay: Send {
    fn render(&self, state: State);
}
