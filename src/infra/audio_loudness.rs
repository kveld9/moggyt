//! Progressive real-time loudness normalizer based on BS.1770 K-weighting.

use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

use super::audio_dsp::{
    DEFAULT_ATTACK_RATE, DEFAULT_MAX_GAIN, DEFAULT_MIN_GAIN, DEFAULT_RELEASE_RATE,
    DEFAULT_WINDOW_SAMPLES, NOISE_GATE_THRESHOLD_DB, TARGET_LUFS_DB, compute_target_gain,
    rms_to_db,
};
use super::audio_kweighting::{KWeighting, LUFS_OFFSET_DB};

/// Progressive real-time loudness normalizer adjusting gain toward a -14.0 dB target.
///
/// # Architecture & Methodological Limits
///
/// **What this implementation IS:**
/// This is a real-time, progressive, single-pass dynamic gain controller designed for
/// streaming audio. Incoming samples are pre-filtered using a per-channel BS.1770
/// K-weighting filter chain ([`KWeighting`]: high-shelf head model + high-pass filter).
/// The filter energy is measured over sliding windows and converted to LUFS using
/// [`LUFS_OFFSET_DB`]. The normalizer smoothly slews the applied gain toward
/// [`TARGET_LUFS_DB`] (-14.0 LUFS) using asymmetrical attack and release ballistics,
/// with safety clamps (`min_gain` / `max_gain`), a noise gate floor at
/// `NOISE_GATE_THRESHOLD_DB` (-60 dBFS), and peak limiting capped at `peak_reference`.
///
/// **What this implementation IS NOT:**
/// This is **NOT** a full ITU-R BS.1770 / EBU R128 integrated loudness normalizer.
/// While the measurement pre-filter is K-weighted, it does not perform a whole-track
/// pre-scan or relative gating, avoiding pre-downloading and start delays in streaming
/// playback. Loudness normalization remains off by default.
pub struct LoudnessNormalizer<I> {
    input: I,
    target_lufs_db: f32,
    pub(crate) current_gain: f32,
    pub(crate) target_gain: f32,
    window_samples: usize,
    pub(crate) sample_count: usize,
    pub(crate) sum_squares: f32,
    min_gain: f32,
    max_gain: f32,
    attack_rate: f32,
    release_rate: f32,
    peak_reference: f32,
    k_weighting: KWeighting,
}

impl<I: Source<Item = f32>> LoudnessNormalizer<I> {
    /// Creates a new `LoudnessNormalizer` with default -14.0 dB target.
    pub fn new(input: I) -> Self {
        Self::with_target(input, TARGET_LUFS_DB)
    }

    /// Creates a new `LoudnessNormalizer` with custom target in dB.
    pub fn with_target(input: I, target_lufs_db: f32) -> Self {
        let k_weighting = KWeighting::new(input.sample_rate(), input.channels());
        Self {
            input,
            target_lufs_db,
            current_gain: 1.0,
            target_gain: 1.0,
            window_samples: DEFAULT_WINDOW_SAMPLES,
            sample_count: 0,
            sum_squares: 0.0,
            min_gain: DEFAULT_MIN_GAIN,
            max_gain: DEFAULT_MAX_GAIN,
            attack_rate: DEFAULT_ATTACK_RATE,
            release_rate: DEFAULT_RELEASE_RATE,
            peak_reference: 1.0,
            k_weighting,
        }
    }

    /// Creates a new `LoudnessNormalizer` with configurable window and slew rates for tests.
    pub fn with_parameters(
        input: I,
        target_lufs_db: f32,
        window_samples: usize,
        attack_rate: f32,
        release_rate: f32,
    ) -> Self {
        let k_weighting = KWeighting::new(input.sample_rate(), input.channels());
        Self {
            input,
            target_lufs_db,
            current_gain: 1.0,
            target_gain: 1.0,
            window_samples: window_samples.max(1),
            sample_count: 0,
            sum_squares: 0.0,
            min_gain: DEFAULT_MIN_GAIN,
            max_gain: DEFAULT_MAX_GAIN,
            attack_rate,
            release_rate,
            peak_reference: 1.0,
            k_weighting,
        }
    }

    /// Returns the active instantaneous gain multiplier applied to samples.
    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    /// Returns the target gain multiplier computed from the most recent window.
    pub fn target_gain(&self) -> f32 {
        self.target_gain
    }

    fn update_target_gain(&mut self) {
        let count = self.sample_count.max(1) as f32;
        let rms = (self.sum_squares / count).sqrt();
        let measured_lufs = rms_to_db(rms) + LUFS_OFFSET_DB;
        if measured_lufs < NOISE_GATE_THRESHOLD_DB {
            return;
        }
        self.target_gain =
            compute_target_gain(measured_lufs, self.target_lufs_db, self.min_gain, self.max_gain);
    }

    fn accumulate_sample(&mut self, sample: f32) {
        let weighted = self.k_weighting.process(sample);
        self.sum_squares += weighted * weighted;
        self.sample_count += 1;
        if self.sample_count >= self.window_samples {
            self.update_target_gain();
            self.sum_squares = 0.0;
            self.sample_count = 0;
        }
    }

    fn ease_gain(&mut self) {
        if self.current_gain > self.target_gain {
            let step = (self.current_gain - self.target_gain).min(self.attack_rate);
            self.current_gain -= step;
        } else if self.current_gain < self.target_gain {
            let step = (self.target_gain - self.current_gain).min(self.release_rate);
            self.current_gain += step;
        }
    }

    fn apply_gain_with_limiting(&self, sample: f32) -> f32 {
        let scaled = sample * self.current_gain;
        scaled.clamp(-self.peak_reference, self.peak_reference)
    }
}

impl<I: Source<Item = f32>> Iterator for LoudnessNormalizer<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;
        self.accumulate_sample(sample);
        self.ease_gain();
        Some(self.apply_gain_with_limiting(sample))
    }
}

impl<I: Source<Item = f32>> Source for LoudnessNormalizer<I> {
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
        self.input.try_seek(pos)?;
        self.current_gain = 1.0; // Previous position's gain estimate must not apply after seek.
        self.target_gain = 1.0;
        self.sample_count = 0;
        self.sum_squares = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db_to_linear;

    /// Deterministic test source yielding a pre-defined vector of samples.
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
    }

    fn sine_samples(amplitude: f32, freq_hz: f32, sample_rate: u32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                amplitude * (2.0 * std::f32::consts::PI * freq_hz * t).sin()
            })
            .collect()
    }

    fn amplitude_for_lufs(target_lufs: f32, freq_hz: f32, sample_rate: u32) -> f32 {
        let mut kw = KWeighting::new(sample_rate, 1);
        let count = 44100;
        let mut out_sq = 0.0_f32;
        let mut measured_count = 0_usize;
        for i in 0..count {
            let t = i as f32 / sample_rate as f32;
            let x = (2.0 * std::f32::consts::PI * freq_hz * t).sin();
            let y = kw.process(x);
            if i >= 2000 {
                out_sq += y * y;
                measured_count += 1;
            }
        }
        let rms_out = (out_sq / measured_count as f32).sqrt();
        let measured_lufs_at_unity = rms_to_db(rms_out) + LUFS_OFFSET_DB;
        db_to_linear(target_lufs - measured_lufs_at_unity)
    }

    #[test]
    fn test_normalizer_converges_gain_toward_target_on_loud_synthetic_buffer() {
        let amp_0dbfs = amplitude_for_lufs(0.0, 1000.0, 44100);
        let input = sine_samples(amp_0dbfs, 1000.0, 44100, 4410);
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 441, 0.05, 0.01);
        assert_eq!(normalizer.current_gain(), 1.0);
        let _output: Vec<f32> = normalizer.by_ref().collect();
        assert!(
            normalizer.current_gain() < 0.5,
            "expected gain to attenuate significantly toward target, got {}",
            normalizer.current_gain()
        );
        let target = normalizer.target_gain();
        let expected_target = db_to_linear(-14.0);
        assert!(
            (target - expected_target).abs() < 1e-3,
            "expected target gain {expected_target}, got {target}"
        );
    }

    #[test]
    fn test_normalizer_leaves_near_target_buffer_near_unity() {
        let near_target_amp = amplitude_for_lufs(-14.0, 1000.0, 44100);
        let input = sine_samples(near_target_amp, 1000.0, 44100, 4410);
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 441, 0.05, 0.01);
        let _output: Vec<f32> = normalizer.by_ref().collect();
        assert!(
            (normalizer.current_gain() - 1.0).abs() < 0.05,
            "expected gain to remain near unity, got {}",
            normalizer.current_gain()
        );
    }

    #[test]
    fn test_normalizer_bounded_no_runaway_gain() {
        let quiet_amp = amplitude_for_lufs(-30.0, 1000.0, 44100);
        let input = sine_samples(quiet_amp, 1000.0, 44100, 4410);
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 441, 0.05, 0.01);
        let _output: Vec<f32> = normalizer.by_ref().collect();
        assert!(normalizer.target_gain() <= DEFAULT_MAX_GAIN);
        assert!(normalizer.current_gain() <= DEFAULT_MAX_GAIN);

        // Sub-noise-gate silence (< -60 dBFS): should not trigger gain increase
        let silent_input = vec![1e-5; 50];
        let silent_source = TestSampleSource::new(silent_input, 1, 44100);
        let mut silent_norm =
            LoudnessNormalizer::with_parameters(silent_source, -14.0, 10, 0.05, 0.01);
        let _silent_out: Vec<f32> = silent_norm.by_ref().collect();
        assert_eq!(silent_norm.target_gain(), 1.0);
    }

    #[test]
    fn test_normalizer_highpass_boosts_low_frequency_more_than_1khz() {
        let amplitude = 0.5;
        let samples_1k = sine_samples(amplitude, 1000.0, 44100, 4410);
        let source_1k = TestSampleSource::new(samples_1k, 1, 44100);
        let mut normalizer_1k =
            LoudnessNormalizer::with_parameters(source_1k, -14.0, 441, 0.05, 0.01);
        let _out_1k: Vec<f32> = normalizer_1k.by_ref().collect();
        let target_1khz = normalizer_1k.target_gain();

        let samples_40hz = sine_samples(amplitude, 40.0, 44100, 4410);
        let source_40hz = TestSampleSource::new(samples_40hz, 1, 44100);
        let mut normalizer_40hz =
            LoudnessNormalizer::with_parameters(source_40hz, -14.0, 441, 0.05, 0.01);
        let _out_40hz: Vec<f32> = normalizer_40hz.by_ref().collect();
        let target_40hz = normalizer_40hz.target_gain();

        assert!(
            target_40hz > target_1khz,
            "expected target gain for 40 Hz ({target_40hz}) > 1 kHz ({target_1khz})"
        );
    }
}
