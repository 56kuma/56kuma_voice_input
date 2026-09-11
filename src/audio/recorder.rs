//! cpal-backed microphone. Thin adapter: it only opens/closes the stream and
//! forwards samples; all conversion lives in `audio_data`.
//!
//! `cpal::Stream` is not `Send`, so the stream lives on a dedicated audio
//! thread and [`CpalRecorder`] talks to it over a command channel.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::audio_data::{rms_level, AudioData};
use super::{AudioError, AudioRecorder};

/// Shared microphone level (`0.0..=1.0` stored as f32 bits) for the overlay.
#[derive(Clone, Default)]
pub struct LevelMeter(Arc<AtomicU32>);

impl LevelMeter {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, level: f32) {
        self.0.store(level.to_bits(), Ordering::Relaxed);
    }
}

enum Command {
    Start(Sender<Result<(), AudioError>>),
    Stop(Sender<Result<AudioData, AudioError>>),
}

pub struct CpalRecorder {
    commands: Sender<Command>,
}

impl CpalRecorder {
    pub fn new(target_rate: u32, level: LevelMeter) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let mut worker = Worker {
                    target_rate,
                    level,
                    active: None,
                };
                for command in rx {
                    match command {
                        Command::Start(reply) => {
                            let _ = reply.send(worker.start());
                        }
                        Command::Stop(reply) => {
                            let _ = reply.send(worker.stop());
                        }
                    }
                }
            })
            .expect("spawn audio thread");
        Self { commands: tx }
    }
}

impl AudioRecorder for CpalRecorder {
    fn start(&mut self) -> Result<(), AudioError> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::Start(reply))
            .map_err(|_| AudioError::Device("audio thread gone".into()))?;
        result
            .recv()
            .unwrap_or_else(|_| Err(AudioError::Device("audio thread gone".into())))
    }

    fn stop(&mut self) -> Result<AudioData, AudioError> {
        let (reply, result) = mpsc::channel();
        self.commands
            .send(Command::Stop(reply))
            .map_err(|_| AudioError::Device("audio thread gone".into()))?;
        result
            .recv()
            .unwrap_or_else(|_| Err(AudioError::Device("audio thread gone".into())))
    }
}

struct ActiveStream {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
}

/// Owns the stream; only ever touched from the audio thread.
struct Worker {
    target_rate: u32,
    level: LevelMeter,
    active: Option<ActiveStream>,
}

impl Worker {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.active.is_some() {
            return Ok(());
        }
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(AudioError::NoDevice)?;
        let supported = device
            .default_input_config()
            .map_err(|e| map_device_error(&e.to_string()))?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let config: cpal::StreamConfig = supported.clone().into();
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
        let err_fn = |e| log::error!("audio stream error: {e}");

        let stream = {
            let buffer = Arc::clone(&buffer);
            let level = self.level.clone();
            let push = move |frames: &[f32]| {
                level.set(rms_level(frames));
                if let Ok(mut buf) = buffer.lock() {
                    buf.extend_from_slice(frames);
                }
            };
            match supported.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &config,
                    move |data: &[f32], _| push(data),
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let frames: Vec<f32> =
                            data.iter().map(|&s| f32::from(s) / 32_768.0).collect();
                        push(&frames);
                    },
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let frames: Vec<f32> = data
                            .iter()
                            .map(|&s| (f32::from(s) - 32_768.0) / 32_768.0)
                            .collect();
                        push(&frames);
                    },
                    err_fn,
                    None,
                ),
                other => {
                    return Err(AudioError::Device(format!(
                        "unsupported sample format {other:?}"
                    )))
                }
            }
            .map_err(|e| map_device_error(&e.to_string()))?
        };
        stream
            .play()
            .map_err(|e| map_device_error(&e.to_string()))?;
        log::info!("microphone opened: {sample_rate} Hz, {channels} ch");
        self.active = Some(ActiveStream {
            stream,
            buffer,
            sample_rate,
            channels,
        });
        Ok(())
    }

    fn stop(&mut self) -> Result<AudioData, AudioError> {
        let active = self.active.take().ok_or(AudioError::NotRecording)?;
        // Dropping the stream releases the device: Idle uses no mic at all.
        drop(active.stream);
        self.level.set(0.0);
        let frames = std::mem::take(&mut *active.buffer.lock().unwrap_or_else(|p| p.into_inner()));
        let audio = AudioData::from_interleaved_f32(active.sample_rate, active.channels, &frames)
            .resampled(self.target_rate);
        log::info!("microphone closed: {:?} captured", audio.duration());
        Ok(audio)
    }
}

fn map_device_error(message: &str) -> AudioError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("permission") || lower.contains("denied") || lower.contains("access") {
        AudioError::PermissionDenied
    } else if lower.contains("no such device") || lower.contains("not found") {
        AudioError::NoDevice
    } else {
        AudioError::Device(message.to_owned())
    }
}
