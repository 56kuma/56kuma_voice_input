//! Shows the top-bar icon and walks it through every state, without touching
//! the hotkey, microphone or network. Use it to check that the desktop has a
//! working tray host:
//!
//! ```bash
//! cargo run --example tray_probe
//! ```

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;
    use voice_input::app::state::State;
    use voice_input::ui::tray::TrayOverlay;
    use voice_input::ui::Overlay;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let tray = TrayOverlay::spawn(Box::new(|| std::process::exit(0)));
    let steps = [
        State::Idle,
        State::Recording,
        State::Transcribing,
        State::Inserting,
        State::Idle, // ✓ flash, then back to the ring on its own
        State::Error,
        State::Idle,
    ];
    for state in steps {
        println!("{state:?}");
        tray.render(state);
        std::thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the tray icon is Linux only");
}
