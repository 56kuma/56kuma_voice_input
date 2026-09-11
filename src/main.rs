//! Entry point: wires the pure controller to the OS adapters.
//!
//! Threads:
//! - main thread: overlay window (eframe) and, on Windows, the hotkey
//!   message loop
//! - "controller": tokio current-thread runtime running the event loop
//! - "audio": owns the cpal stream while recording
//! - "hotkey-listener"/"hotkey-portal": forwards hotkey presses

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use voice_input::app::controller::{Controller, Dependencies};
use voice_input::app::state::State;
use voice_input::audio::recorder::{CpalRecorder, LevelMeter};
use voice_input::config::credentials::{
    resolve_api_key, CredentialStore, KeyringStore, OPENAI_ACCOUNT,
};
use voice_input::config::AppConfig;
use voice_input::hotkey::{Debouncer, Hotkey};
use voice_input::transcription::openai::OpenAiTranscriptionProvider;
use voice_input::transcription::TranscriptionProvider;
use voice_input::ui::overlay::{self, OverlayHandle, OverlayModel};
use voice_input::ui::Overlay;

const USAGE: &str = "\
voice_input — press Ctrl+Space, speak, press Ctrl+Space again.

USAGE:
    voice_input                 run in the background with the overlay
    voice_input --set-api-key   read an API key from stdin and store it in
                                the OS credential store
    voice_input --delete-api-key
    voice_input --config-path   print where config.toml is read from
    voice_input --help | --version

Logging: RUST_LOG=debug voice_input  (never logs keys, audio, or text)
";

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => print!("{USAGE}"),
        Some("--version" | "-V") => println!("voice_input {}", env!("CARGO_PKG_VERSION")),
        Some("--config-path") => match AppConfig::default_path() {
            Some(p) => println!("{}", p.display()),
            None => println!("(no config directory on this platform)"),
        },
        Some("--set-api-key") => set_api_key(),
        Some("--delete-api-key") => match KeyringStore.delete(OPENAI_ACCOUNT) {
            Ok(()) => println!("API key removed from the credential store."),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        },
        Some(other) => {
            eprintln!("unknown option {other}\n{USAGE}");
            std::process::exit(2);
        }
        None => run(),
    }
}

fn set_api_key() {
    eprintln!("Paste the API key and press Enter (input is not echoed to logs):");
    let mut key = String::new();
    if std::io::stdin().read_line(&mut key).is_err() || key.trim().is_empty() {
        eprintln!("error: no key given");
        std::process::exit(1);
    }
    match KeyringStore.set(OPENAI_ACCOUNT, key.trim()) {
        Ok(()) => println!("API key stored in the OS credential store."),
        Err(e) => {
            eprintln!(
                "error: {e}\nFallback: set the {} environment variable.",
                voice_input::config::credentials::OPENAI_ENV_VAR
            );
            std::process::exit(1);
        }
    }
}

fn run() {
    let config = match AppConfig::load() {
        Ok(c) => c,
        Err(e) => {
            log::error!("{e}; using defaults");
            AppConfig::default()
        }
    };

    let hotkey = Hotkey::parse(&config.hotkey).unwrap_or_else(|e| {
        log::error!("{e}; using Ctrl+Space");
        Hotkey::default_toggle()
    });

    let api_key = resolve_api_key(
        &KeyringStore,
        |name| std::env::var(name).ok(),
        config.openai.api_key.as_deref(),
    );
    if api_key.is_none() {
        log::error!(
            "no API key: run `voice_input --set-api-key` or set OPENAI_API_KEY; \
             transcription will fail until then"
        );
    }
    let provider: Arc<dyn TranscriptionProvider> = match config.provider.as_str() {
        "openai" => Arc::new(OpenAiTranscriptionProvider::new(
            api_key,
            config.openai_config(),
        )),
        other => {
            log::error!("unknown provider {other:?}; using openai");
            Arc::new(OpenAiTranscriptionProvider::new(
                api_key,
                config.openai_config(),
            ))
        }
    };

    let level = LevelMeter::default();
    let model = OverlayModel::new(level.clone());
    let overlay = OverlayHandle(Arc::clone(&model));

    let controller = Controller::new(Dependencies {
        recorder: Box::new(CpalRecorder::new(config.audio.sample_rate, level)),
        provider,
        injector: voice_input::platform::text_injector(),
        overlay: Box::new(overlay.clone()),
        error_display: Duration::from_secs(config.error_display_secs.max(1)),
    });
    let events = controller.sender();

    std::thread::Builder::new()
        .name("controller".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let mut controller = controller;
            runtime.block_on(controller.run());
        })
        .expect("spawn controller thread");

    // Hotkey registration happens once the UI thread is up (Windows needs
    // the win32 message loop on the registering thread).
    let overlay_for_hotkey = overlay.clone();
    let hotkey_label = hotkey.to_string();
    let on_ready: Box<dyn FnOnce() + Send> = Box::new(move || {
        let debouncer = Mutex::new(Debouncer::new(Duration::from_millis(250)));
        let on_toggle = Box::new(move || {
            if debouncer.lock().unwrap().accept(Instant::now()) {
                events.hotkey_toggled();
            }
        });
        let registered = voice_input::platform::hotkey_provider()
            .and_then(|mut provider| provider.register(&hotkey, on_toggle));
        if let Err(e) = registered {
            // Never fail silently: log it and pin the overlay to Error.
            log::error!("HOTKEY UNAVAILABLE: {e}");
            log::error!("voice input cannot be triggered; fix the above and restart");
            overlay_for_hotkey.render(State::Error);
        }
    });

    log::info!(
        "voice_input {} ready: press {hotkey_label} to start",
        env!("CARGO_PKG_VERSION")
    );
    if let Err(e) = overlay::run(model, config.overlay.clone(), on_ready) {
        log::error!("overlay failed: {e}");
        std::process::exit(1);
    }
}
