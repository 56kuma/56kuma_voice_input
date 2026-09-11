//! Diagnostic: registers Ctrl+Space with the platform provider and prints
//! TOGGLE for 6 seconds. `cargo run --example hotkey_probe`
use voice_input::hotkey::Hotkey;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    let mut provider = voice_input::platform::hotkey_provider().expect("provider");
    provider
        .register(&Hotkey::default_toggle(), Box::new(|| println!("TOGGLE")))
        .expect("register");
    std::thread::sleep(std::time::Duration::from_secs(6));
    println!("done");
}
