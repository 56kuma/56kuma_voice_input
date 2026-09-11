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

#[cfg(test)]
mod tests {
    use super::*;

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
