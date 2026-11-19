//! Audible level tap: publishes the RMS of the samples that pass through the playback chain.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

/// Number of samples averaged into one published level value.
pub const LEVEL_TAP_WINDOW_SAMPLES: usize = 1024;

/// Source wrapper that publishes the RMS of the samples it passes through.
pub struct LevelTap<I> {
    input: I,
    level: Arc<AtomicU32>,
    sum_squares: f32,
    window_samples: usize,
}

impl<I: Source<Item = f32>> LevelTap<I> {
    /// Wraps `input`, publishing one RMS value per [`LEVEL_TAP_WINDOW_SAMPLES`] samples.
    pub fn new(input: I, level: Arc<AtomicU32>) -> Self {
        Self { input, level, sum_squares: 0.0, window_samples: 0 }
    }

    fn publish_if_ready(&mut self) {
        if self.window_samples < LEVEL_TAP_WINDOW_SAMPLES {
            return;
        }
        let rms = (self.sum_squares / self.window_samples as f32).sqrt();
        self.level.store(rms.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.sum_squares = 0.0;
        self.window_samples = 0;
    }
}

impl<I: Source<Item = f32>> Iterator for LevelTap<I> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.input.next()?;
        self.sum_squares += sample * sample;
        self.window_samples += 1;
        self.publish_if_ready();
        Some(sample)
    }
}

impl<I: Source<Item = f32>> Source for LevelTap<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.input.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSampleSource {
        samples: std::vec::IntoIter<f32>,
        channels: u16,
        sample_rate: u32,
    }

    impl TestSampleSource {
        fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
            Self { samples: samples.into_iter(), channels, sample_rate }
        }
    }

    impl Iterator for TestSampleSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            self.samples.next()
        }
    }

    impl Source for TestSampleSource {
        fn current_span_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> u16 {
            self.channels
        }

        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            None
        }

        fn try_seek(&mut self, _pos: Duration) -> Result<(), SeekError> {
            Ok(())
        }
    }

    #[test]
    fn level_tap_publishes_rms_after_one_window() {
        let level = Arc::new(AtomicU32::new(0));
        let samples = vec![0.5_f32; LEVEL_TAP_WINDOW_SAMPLES];
        let source = TestSampleSource::new(samples.clone(), 1, 44100);
        let tap = LevelTap::new(source, Arc::clone(&level));

        let collected: Vec<f32> = tap.collect();

        assert_eq!(collected, samples);
        let published = f32::from_bits(level.load(Ordering::Relaxed));
        assert!((published - 0.5).abs() < 1e-4);
    }

    #[test]
    fn level_tap_publishes_zero_for_silence() {
        let level = Arc::new(AtomicU32::new(0));
        let samples = vec![0.0_f32; LEVEL_TAP_WINDOW_SAMPLES];
        let source = TestSampleSource::new(samples, 1, 44100);
        let tap = LevelTap::new(source, Arc::clone(&level));

        let _: Vec<f32> = tap.collect();

        assert_eq!(f32::from_bits(level.load(Ordering::Relaxed)), 0.0);
    }
}
