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
    /// sound card (any channel count). Channels are averaged, but a channel
    /// whose polarity is inverted relative to the first (common on laptop
    /// mic arrays) is flipped first so the voice doesn't cancel itself out.
    pub fn from_interleaved_f32(sample_rate: u32, channels: u16, frames: &[f32]) -> Self {
        let channels = usize::from(channels.max(1));
        let signs: Vec<f32> = (0..channels)
            .map(|c| {
                let dot: f64 = frames
                    .chunks_exact(channels)
                    .map(|frame| f64::from(frame[0]) * f64::from(frame[c]))
                    .sum();
                if dot < 0.0 {
                    log::info!("mic channel {c} is phase-inverted; flipping it before downmix");
                    -1.0
                } else {
                    1.0
                }
            })
            .collect();
        let samples = frames
            .chunks(channels)
            .map(|frame| {
                frame
                    .iter()
                    .zip(&signs)
                    .map(|(s, sign)| s * sign)
                    .sum::<f32>()
                    / frame.len() as f32
            })
            .map(f32_to_i16)
            .collect();
        Self::new(sample_rate, samples)
    }

    /// Band-limited (windowed-sinc) resample to `target_rate`. When
    /// downsampling, the kernel also low-passes just below the new Nyquist,
    /// so hiss and sibilant energy above it is removed instead of aliasing
    /// back into the speech band.
    pub fn resampled(&self, target_rate: u32) -> Self {
        if target_rate == self.sample_rate || self.samples.is_empty() || target_rate == 0 {
            return Self::new(target_rate, self.samples.clone());
        }
        let ratio = self.sample_rate as f64 / target_rate as f64;
        let kernel = SincKernel::new(ratio);
        let out_len = (self.samples.len() as f64 / ratio).round() as usize;
        let last = self.samples.len() as isize - 1;
        let samples = (0..out_len)
            .map(|i| {
                let pos = i as f64 * ratio;
                let first = ((pos - kernel.half_width).ceil() as isize).max(0);
                let end = ((pos + kernel.half_width).floor() as isize).min(last);
                let (mut acc, mut weight_sum) = (0.0, 0.0);
                for n in first..=end {
                    let w = kernel.at(pos - n as f64);
                    acc += w * f64::from(self.samples[n as usize]);
                    weight_sum += w;
                }
                // Normalising by the weights actually used keeps the level
                // right at the edges, where the kernel is cut off.
                let value = if weight_sum.abs() > 1e-9 {
                    acc / weight_sum
                } else {
                    0.0
                };
                value
                    .round()
                    .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
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

/// Blackman-windowed sinc low-pass, tabulated so resampling a long
/// recording doesn't evaluate trig functions per tap.
struct SincKernel {
    half_width: f64,
    table: Vec<f64>,
}

impl SincKernel {
    /// Zero crossings of the sinc on each side: sharper cutoff vs. cost.
    const ZERO_CROSSINGS: f64 = 16.0;
    /// Table entries per input sample.
    const RESOLUTION: f64 = 256.0;
    /// Cutoff as a fraction of the lower Nyquist, leaving a transition band.
    const CUTOFF: f64 = 0.95;

    /// `ratio` = source rate / target rate.
    fn new(ratio: f64) -> Self {
        // Cutoff in cycles per input sample (0.5 = source Nyquist).
        let fc = 0.5 * (1.0 / ratio).min(1.0) * Self::CUTOFF;
        let half_width = Self::ZERO_CROSSINGS / (2.0 * fc);
        let len = (half_width * Self::RESOLUTION).ceil() as usize + 1;
        let table = (0..len)
            .map(|i| {
                let x = i as f64 / Self::RESOLUTION;
                let u = (x / half_width).min(1.0);
                let window = 0.42
                    + 0.5 * (std::f64::consts::PI * u).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * u).cos();
                let arg = 2.0 * fc * x;
                let sinc = if arg == 0.0 {
                    1.0
                } else {
                    (std::f64::consts::PI * arg).sin() / (std::f64::consts::PI * arg)
                };
                sinc * window
            })
            .collect();
        Self { half_width, table }
    }

    /// Kernel value at `x` input samples from the centre.
    fn at(&self, x: f64) -> f64 {
        let index = (x.abs() * Self::RESOLUTION).round() as usize;
        self.table.get(index).copied().unwrap_or(0.0)
    }
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_are_averaged_into_mono_i16() {
        let audio = AudioData::from_interleaved_f32(48_000, 2, &[1.0, 0.0, 0.5, 0.5]);

        assert_eq!(audio.sample_rate(), 48_000);
        assert_eq!(audio.samples(), &[16_383, 16_383]);
    }

    #[test]
    fn a_phase_inverted_channel_is_flipped_instead_of_cancelling_the_voice() {
        let frames: Vec<f32> = [0.5f32, -0.25, 0.4].iter().flat_map(|&s| [s, -s]).collect();

        let audio = AudioData::from_interleaved_f32(48_000, 2, &frames);

        assert_eq!(audio.samples(), &[16_383, -8_191, 13_106]);
    }

    fn sine(rate: u32, freq: f64, secs: f64, amplitude: f64) -> AudioData {
        let n = (f64::from(rate) * secs) as usize;
        let samples = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (amplitude * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
            })
            .collect();
        AudioData::new(rate, samples)
    }

    /// RMS of the middle half, away from the edges.
    fn middle_rms(audio: &AudioData) -> f64 {
        let s = audio.samples();
        let mid = &s[s.len() / 4..s.len() * 3 / 4];
        (mid.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / mid.len() as f64).sqrt()
    }

    #[test]
    fn speech_band_tones_pass_through_downsampling_at_full_level() {
        let input = sine(44_100, 1_000.0, 0.5, 10_000.0);

        let out = input.resampled(16_000);

        let ratio = middle_rms(&out) / middle_rms(&input);
        assert!((ratio - 1.0).abs() < 0.02, "1 kHz level ratio {ratio}");
    }

    #[test]
    fn tones_above_the_new_nyquist_are_removed_not_aliased() {
        // 10 kHz at 44.1 kHz would fold to 6 kHz at 16 kHz without a low-pass.
        let input = sine(44_100, 10_000.0, 0.5, 10_000.0);

        let out = input.resampled(16_000);

        let ratio = middle_rms(&out) / middle_rms(&input);
        assert!(ratio < 0.01, "10 kHz leaked through at ratio {ratio}");
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
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16_000, 16)
        );
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
