//! In-memory PCM audio. Mono 16-bit, arbitrary sample rate.

use std::time::Duration;

/// Recorded audio, kept only in memory and dropped after transcription.
#[derive(Clone, PartialEq, Eq)]
pub struct AudioData {
    sample_rate: u32,
    samples: Vec<i16>,
}

impl std::fmt::Debug for AudioData {
    /// Never print the samples themselves (privacy + log noise).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioData")
            .field("sample_rate", &self.sample_rate)
            .field("samples", &self.samples.len())
            .finish()
    }
}

impl AudioData {
    pub fn new(sample_rate: u32, samples: Vec<i16>) -> Self {
        Self {
            sample_rate,
            samples,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    /// Builds mono audio from interleaved f32 frames as delivered by the
    /// sound card (any channel count). Channels are averaged.
    pub fn from_interleaved_f32(sample_rate: u32, channels: u16, frames: &[f32]) -> Self {
        let channels = usize::from(channels.max(1));
        let samples = frames
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .map(f32_to_i16)
            .collect();
        Self::new(sample_rate, samples)
    }

    /// Linear-interpolation resample to `target_rate`. Good enough for
    /// speech; avoids pulling in a DSP crate.
    pub fn resampled(&self, target_rate: u32) -> Self {
        if target_rate == self.sample_rate || self.samples.is_empty() || target_rate == 0 {
            return Self::new(target_rate, self.samples.clone());
        }
        let ratio = self.sample_rate as f64 / target_rate as f64;
        let out_len = (self.samples.len() as f64 / ratio).round() as usize;
        let last = self.samples.len() - 1;
        let samples = (0..out_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let left = (pos.floor() as usize).min(last);
                let right = (left + 1).min(last);
                let frac = pos - left as f64;
                let a = f64::from(self.samples[left]);
                let b = f64::from(self.samples[right]);
                (a + (b - a) * frac).round() as i16
            })
            .collect();
        Self::new(target_rate, samples)
    }

    /// 16-bit PCM WAV, in memory. Nothing is written to disk.
    pub fn to_wav(&self) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = std::io::Cursor::new(Vec::with_capacity(44 + self.samples.len() * 2));
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("in-memory wav");
            for &s in &self.samples {
                writer.write_sample(s).expect("in-memory wav");
            }
            writer.finalize().expect("in-memory wav");
        }
        cursor.into_inner()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration(&self) -> Duration {
        if self.sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.samples.len() as f64 / self.sample_rate as f64)
    }
}

/// Root-mean-square level of a chunk of f32 samples, clamped to `0.0..=1.0`.
/// Used only to drive the overlay's pulse while recording.
pub fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    mean_square.sqrt().clamp(0.0, 1.0)
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_are_averaged_into_mono_i16() {
        let audio = AudioData::from_interleaved_f32(48_000, 2, &[1.0, -1.0, 0.5, 0.5]);

        assert_eq!(audio.sample_rate(), 48_000);
        assert_eq!(audio.samples(), &[0, 16_383]);
    }

    #[test]
    fn out_of_range_floats_are_clipped_not_wrapped() {
        let audio = AudioData::from_interleaved_f32(16_000, 1, &[2.0, -2.0]);

        assert_eq!(audio.samples(), &[i16::MAX, -i16::MAX]);
    }

    #[test]
    fn resampling_48k_to_16k_keeps_duration_and_level() {
        let audio = AudioData::new(48_000, vec![1000; 48_000]);

        let out = audio.resampled(16_000);

        assert_eq!(out.sample_rate(), 16_000);
        assert_eq!(out.samples().len(), 16_000);
        assert!(out.samples().iter().all(|&s| s == 1000));
    }

    #[test]
    fn resampling_to_the_same_rate_is_identity() {
        let audio = AudioData::new(16_000, vec![1, 2, 3, 4]);

        assert_eq!(audio.resampled(16_000), audio);
    }

    #[test]
    fn wav_round_trips_through_a_wav_reader() {
        let audio = AudioData::new(16_000, vec![0, 100, -100, i16::MAX]);

        let bytes = audio.to_wav();

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(samples, audio.samples());
    }

    #[test]
    fn rms_level_is_zero_for_silence_and_one_for_full_scale() {
        assert_eq!(rms_level(&[]), 0.0);
        assert_eq!(rms_level(&[0.0; 8]), 0.0);
        assert_eq!(rms_level(&[1.0, -1.0, 1.0, -1.0]), 1.0);
        assert!((rms_level(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
        assert_eq!(rms_level(&[3.0, -3.0]), 1.0, "clamped");
    }

    #[test]
    fn audio_without_samples_is_empty() {
        let audio = AudioData::new(16_000, vec![]);
        assert!(audio.is_empty());
    }

    #[test]
    fn one_second_of_16khz_audio_has_one_second_duration() {
        let audio = AudioData::new(16_000, vec![0; 16_000]);
        assert!(!audio.is_empty());
        assert_eq!(audio.duration(), Duration::from_secs(1));
    }
}
