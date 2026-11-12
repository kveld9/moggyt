use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

use super::*;

struct RecordingSource {
    samples: std::vec::IntoIter<f32>,
    channels: u16,
    sample_rate: u32,
    recorded_seeks: Arc<Mutex<Vec<Duration>>>,
}

impl RecordingSource {
    fn new(samples: Vec<f32>, recorded_seeks: Arc<Mutex<Vec<Duration>>>) -> Self {
        Self { samples: samples.into_iter(), channels: 1, sample_rate: 44100, recorded_seeks }
    }
}

impl Iterator for RecordingSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.samples.next()
    }
}

impl Source for RecordingSource {
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

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.recorded_seeks.lock().expect("mutex lock").push(pos);
        Ok(())
    }
}

struct FailingSeekSource {
    channels: u16,
    sample_rate: u32,
}

impl FailingSeekSource {
    fn new() -> Self {
        Self { channels: 1, sample_rate: 44100 }
    }
}

impl Iterator for FailingSeekSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

impl Source for FailingSeekSource {
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
        Err(SeekError::NotSupported { underlying_source: "test" })
    }
}

#[test]
fn skip_silence_forwards_seek_and_resets_state() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let inner = RecordingSource::new(vec![0.1, 0.2], Arc::clone(&recorded));
    let mut skipper = SkipSilence::new(inner);
    skipper.leading_trimmed = false;
    skipper.silence_frames.push_back(0.0);
    skipper.output_queue.push_back(0.5);
    skipper.exhausted = true;

    let result = skipper.try_seek(Duration::from_secs(5));
    assert!(result.is_ok());
    assert_eq!(*recorded.lock().expect("lock"), vec![Duration::from_secs(5)]);
    assert!(!skipper.exhausted);
    assert!(skipper.leading_trimmed);
    assert!(skipper.silence_frames.is_empty());
    assert!(skipper.output_queue.is_empty());
}

#[test]
fn static_gain_forwards_seek_and_keeps_samples_flowing() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let inner = RecordingSource::new(vec![0.4, 0.8], Arc::clone(&recorded));
    let mut gain_source = StaticGain::new(inner, 0.5);

    let result = gain_source.try_seek(Duration::from_secs(3));
    assert!(result.is_ok());
    assert_eq!(*recorded.lock().expect("lock"), vec![Duration::from_secs(3)]);
    assert_eq!(gain_source.next(), Some(0.2));
}

#[test]
fn loudness_normalizer_forwards_seek_and_resets_gain_state() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let inner = RecordingSource::new(vec![0.5], Arc::clone(&recorded));
    let mut normalizer = LoudnessNormalizer::new(inner);
    normalizer.current_gain = 0.25;
    normalizer.target_gain = 0.25;
    normalizer.sample_count = 99;
    normalizer.sum_squares = 1.5;

    let result = normalizer.try_seek(Duration::from_secs(7));
    assert!(result.is_ok());
    assert_eq!(*recorded.lock().expect("lock"), vec![Duration::from_secs(7)]);
    assert_eq!(normalizer.current_gain, 1.0);
    assert_eq!(normalizer.target_gain, 1.0);
    assert_eq!(normalizer.sample_count, 0);
    assert_eq!(normalizer.sum_squares, 0.0);
}

#[test]
fn seek_errors_propagate_through_every_wrapper() {
    let mut skipper = SkipSilence::new(FailingSeekSource::new());
    let err = skipper.try_seek(Duration::from_secs(1)).unwrap_err();
    assert!(matches!(err, SeekError::NotSupported { .. }));

    let mut gain = StaticGain::new(FailingSeekSource::new(), 0.5);
    let err = gain.try_seek(Duration::from_secs(1)).unwrap_err();
    assert!(matches!(err, SeekError::NotSupported { .. }));

    let mut normalizer = LoudnessNormalizer::new(FailingSeekSource::new());
    let err = normalizer.try_seek(Duration::from_secs(1)).unwrap_err();
    assert!(matches!(err, SeekError::NotSupported { .. }));
}
