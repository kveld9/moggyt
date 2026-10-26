//! Digital Signal Processing (DSP) filters for audio playback.
//!
//! Provides pure, testable DSP math and rodio [`Source`] wrappers for:
//! 1. [`SkipSilence`]: Trims leading silence and skips sustained trailing silence.
//! 2. [`LoudnessNormalizer`]: Progressive -14 LUFS loudness normalization (first-order approximation).

use std::collections::VecDeque;
use std::time::Duration;

use rodio::Source;

/// Default silence threshold in linear amplitude (~-54 dBFS).
///
/// Threshold Choice:
/// Digital silence in 16-bit or 24-bit PCM rarely sits at true bitwise zero due to
/// dither noise, ADC noise floor, or DC bias. A threshold of 0.002 (-54 dBFS)
/// provides robust discrimination between background noise/dither and intentional
/// quiet passages (such as pianissimo in classical recordings, which typically
/// registers between -40 and -45 dBFS).
pub const DEFAULT_SILENCE_THRESHOLD_AMPLITUDE: f32 = 0.002;

/// Default minimum trailing silence duration in seconds before considering it dead space.
///
/// Threshold Choice:
/// Musical phrasing frequently includes dramatic pauses or reverb ring-outs of
/// 0.5 to 1.5 seconds. Only sustained silence >= 2.0 seconds at the end of a track
/// is treated as trailing gap to be skipped.
pub const DEFAULT_MIN_TRAILING_SILENCE_SECS: f32 = 2.0;

/// Default target loudness reference level in dB (corresponds to -14 LUFS).
///
/// Target Choice:
/// -14 LUFS is the standard target loudness used by YouTube Music, Spotify, and TIDAL
/// to ensure consistent playback levels across tracks without excessive compression.
pub const TARGET_LUFS_DB: f32 = -14.0;

/// Default window size in samples for RMS estimation (~46ms at 44.1 kHz).
pub const DEFAULT_WINDOW_SAMPLES: usize = 2048;

/// Minimum permitted gain multiplier (-20 dBFS attenuation ceiling).
pub const DEFAULT_MIN_GAIN: f32 = 0.1;

/// Maximum permitted gain multiplier (+9.5 dBFS boost ceiling) to prevent runaway gain on quiet tracks.
pub const DEFAULT_MAX_GAIN: f32 = 3.0;

/// Slew rate for reducing gain (attack phase).
/// Fast response to avoid clipping on sudden loud transients.
pub const DEFAULT_ATTACK_RATE: f32 = 0.01;

/// Slew rate for increasing gain (release phase).
/// Slower response to avoid audible "breathing" or "pumping" modulation.
pub const DEFAULT_RELEASE_RATE: f32 = 0.002;

/// Silence noise gate threshold below which gain is not boosted (-60 dBFS).
pub const NOISE_GATE_THRESHOLD_DB: f32 = -60.0;

/// Converts decibels to a linear amplitude factor.
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Converts a linear amplitude factor to decibels, clamped to a floor of -180 dBFS.
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 1e-9 { -180.0 } else { 20.0 * linear.log10() }
}

/// Returns true if `sample` absolute amplitude is below `threshold`.
pub fn is_silent_sample(sample: f32, threshold: f32) -> bool {
    sample.abs() < threshold
}

/// Returns true if all channel samples in `frame` are below `threshold`.
pub fn is_silent_frame(frame: &[f32], threshold: f32) -> bool {
    frame.iter().all(|&s| s.abs() < threshold)
}

/// Computes the Root Mean Square (RMS) energy of a sample buffer.
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Converts an RMS amplitude value to decibels relative to full scale.
pub fn rms_to_db(rms: f32) -> f32 {
    linear_to_db(rms)
}

/// Computes the target gain required to normalize `measured_db` to `target_db`, clamped to bounds.
pub fn compute_target_gain(measured_db: f32, target_db: f32, min_gain: f32, max_gain: f32) -> f32 {
    let delta_db = target_db - measured_db;
    let raw = db_to_linear(delta_db);
    raw.clamp(min_gain, max_gain)
}

/// Rodio [`Source`] wrapper that trims leading silence and skips sustained trailing silence.
///
/// Leading silence below [`DEFAULT_SILENCE_THRESHOLD_AMPLITUDE`] is dropped immediately until
/// audible content begins. Trailing silence that persists for at least
/// [`DEFAULT_MIN_TRAILING_SILENCE_SECS`] is discarded at stream conclusion, while brief
/// musical pauses within the track are preserved.
pub struct SkipSilence<I> {
    input: I,
    channels: usize,
    sample_rate: u32,
    threshold: f32,
    min_trailing_silence_frames: usize,
    leading_trimmed: bool,
    silence_frames: VecDeque<f32>,
    output_queue: VecDeque<f32>,
    exhausted: bool,
}

impl<I: Source<Item = f32>> SkipSilence<I> {
    /// Creates a new `SkipSilence` wrapper with default threshold and duration.
    pub fn new(input: I) -> Self {
        Self::with_thresholds(
            input,
            DEFAULT_SILENCE_THRESHOLD_AMPLITUDE,
            DEFAULT_MIN_TRAILING_SILENCE_SECS,
        )
    }

    /// Creates a new `SkipSilence` wrapper with custom threshold and duration.
    pub fn with_thresholds(input: I, threshold: f32, min_trailing_silence_secs: f32) -> Self {
        let sample_rate = input.sample_rate().max(1);
        let min_trailing_silence_frames =
            (min_trailing_silence_secs * sample_rate as f32).round() as usize;
        Self::with_frame_count(input, threshold, min_trailing_silence_frames)
    }

    /// Creates a new `SkipSilence` wrapper with explicit frame count limit for tests.
    pub fn with_frame_count(input: I, threshold: f32, min_trailing_silence_frames: usize) -> Self {
        let channels = input.channels().max(1) as usize;
        let sample_rate = input.sample_rate().max(1);
        Self {
            input,
            channels,
            sample_rate,
            threshold,
            min_trailing_silence_frames,
            leading_trimmed: false,
            silence_frames: VecDeque::new(),
            output_queue: VecDeque::new(),
            exhausted: false,
        }
    }

    fn read_frame(&mut self) -> Option<Vec<f32>> {
        let first = self.input.next()?;
        let mut frame = Vec::with_capacity(self.channels);
        frame.push(first);
        for _ in 1..self.channels {
            match self.input.next() {
                Some(s) => frame.push(s),
                None => break,
            }
        }
        Some(frame)
    }

    fn handle_leading_frame(&mut self, frame: Vec<f32>, is_silent: bool) -> bool {
        if is_silent {
            return false;
        }
        self.leading_trimmed = true;
        self.output_queue.extend(frame);
        true
    }

    fn buffer_silent_frame(&mut self, frame: Vec<f32>) {
        let max_samples = self.min_trailing_silence_frames * self.channels;
        if self.silence_frames.len() < max_samples {
            self.silence_frames.extend(frame);
        }
    }

    fn flush_silence_before_audible(&mut self) {
        if !self.silence_frames.is_empty() {
            self.output_queue.append(&mut self.silence_frames);
        }
    }

    fn handle_audible_stream_frame(&mut self, frame: Vec<f32>, is_silent: bool) -> bool {
        if is_silent {
            self.buffer_silent_frame(frame);
            return false;
        }
        self.flush_silence_before_audible();
        self.output_queue.extend(frame);
        true
    }

    fn handle_frame(&mut self, frame: Vec<f32>, is_silent: bool) -> bool {
        if !self.leading_trimmed {
            return self.handle_leading_frame(frame, is_silent);
        }
        self.handle_audible_stream_frame(frame, is_silent)
    }

    fn handle_stream_end(&mut self) -> Option<f32> {
        self.exhausted = true;
        let silent_frames = self.silence_frames.len() / self.channels.max(1);
        if silent_frames < self.min_trailing_silence_frames {
            self.output_queue.append(&mut self.silence_frames);
        } else {
            self.silence_frames.clear();
        }
        self.output_queue.pop_front()
    }

    fn pull_next_sample(&mut self) -> Option<f32> {
        while let Some(frame) = self.read_frame() {
            let is_silent = is_silent_frame(&frame, self.threshold);
            if self.handle_frame(frame, is_silent) {
                return self.output_queue.pop_front();
            }
        }
        self.handle_stream_end()
    }
}

impl<I: Source<Item = f32>> Iterator for SkipSilence<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.output_queue.pop_front() {
            return Some(sample);
        }
        if self.exhausted {
            return None;
        }
        self.pull_next_sample()
    }
}

impl<I: Source<Item = f32>> Source for SkipSilence<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.channels as u16
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

/// Progressive real-time loudness normalizer adjusting gain toward a -14.0 dB target.
///
/// # Architecture & Methodological Limits (First-Order Approximation)
///
/// **What this implementation IS:**
/// This is a real-time, progressive, single-pass dynamic gain controller designed for
/// streaming audio. It calculates the Root Mean Square (RMS) energy over sliding windows,
/// computes the decibel deviation from [`TARGET_LUFS_DB`] (-14.0 dB), and smoothly slews
/// the applied gain using asymmetrical attack and release ballistics. It includes safety
/// clamping (`min_gain` / `max_gain`), a noise gate floor at -60 dBFS (preventing the
/// amplification of silence), and peak limiting capped at `peak_reference`.
///
/// **What this implementation IS NOT:**
/// This is **NOT** a full ITU-R BS.1770 / EBU R128 integrated loudness normalizer.
/// Full ITU-R BS.1770 compliance requires:
/// 1. Dual-stage K-weighting filtering (high-pass head model + high-shelf filter).
/// 2. Overlapping 400ms rectangular time windows with 75% overlap.
/// 3. Two-stage gating (absolute gating at -70 LKFS and relative gating at -10 LKFS).
/// 4. An integrated measurement across the **entire track** from start to finish before
///    deriving a single, fixed static gain offset applied across the whole song.
///
/// In a progressive, low-latency streaming pipeline (such as streaming from `yt-dlp`),
/// a full BS.1770 pre-scan cannot be performed without pre-downloading and decoding the
/// entire audio file into memory or disk before playback can start, which would introduce
/// multi-second start delays (violating requirements NF-1 and NF-2). Hence, this module
/// implements an honest, first-order progressive RMS AGC approximation.
pub struct LoudnessNormalizer<I> {
    input: I,
    target_lufs_db: f32,
    current_gain: f32,
    target_gain: f32,
    window_samples: usize,
    sample_count: usize,
    sum_squares: f32,
    min_gain: f32,
    max_gain: f32,
    attack_rate: f32,
    release_rate: f32,
    peak_reference: f32,
}

impl<I: Source<Item = f32>> LoudnessNormalizer<I> {
    /// Creates a new `LoudnessNormalizer` with default -14.0 dB target.
    pub fn new(input: I) -> Self {
        Self::with_target(input, TARGET_LUFS_DB)
    }

    /// Creates a new `LoudnessNormalizer` with custom target in dB.
    pub fn with_target(input: I, target_lufs_db: f32) -> Self {
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
        let measured_db = rms_to_db(rms);
        if measured_db < NOISE_GATE_THRESHOLD_DB {
            return;
        }
        self.target_gain =
            compute_target_gain(measured_db, self.target_lufs_db, self.min_gain, self.max_gain);
    }

    fn accumulate_sample(&mut self, sample: f32) {
        self.sum_squares += sample * sample;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_pure_function_threshold_table() {
        assert!((db_to_linear(0.0) - 1.0).abs() < f32::EPSILON);
        assert!((db_to_linear(-20.0) - 0.1).abs() < 1e-4);
        assert!((linear_to_db(1.0) - 0.0).abs() < f32::EPSILON);
        assert!((linear_to_db(0.1) - (-20.0)).abs() < 1e-4);
        assert_eq!(linear_to_db(0.0), -180.0);

        assert!(is_silent_sample(0.001, 0.002));
        assert!(!is_silent_sample(0.003, 0.002));

        assert!(is_silent_frame(&[0.001, 0.0015], 0.002));
        assert!(!is_silent_frame(&[0.001, 0.05], 0.002));

        assert_eq!(calculate_rms(&[]), 0.0);
        assert!((calculate_rms(&[1.0, -1.0]) - 1.0).abs() < f32::EPSILON);
        assert!((calculate_rms(&[0.0, 0.0]) - 0.0).abs() < f32::EPSILON);

        assert!((rms_to_db(1.0) - 0.0).abs() < f32::EPSILON);

        let gain_atten = compute_target_gain(0.0, -14.0, 0.1, 3.0);
        assert!((gain_atten - db_to_linear(-14.0)).abs() < 1e-4);

        let gain_max_clamped = compute_target_gain(-40.0, -14.0, 0.1, 3.0);
        assert_eq!(gain_max_clamped, 3.0);

        let gain_min_clamped = compute_target_gain(20.0, -14.0, 0.1, 3.0);
        assert_eq!(gain_min_clamped, 0.1);
    }

    #[test]
    fn test_skip_silence_trims_leading_silence() {
        let input = vec![0.0, 0.0005, 0.001, 0.5, 0.6, 0.7];
        let source = TestSampleSource::new(input, 1, 44100);
        let skipper = SkipSilence::with_frame_count(source, 0.002, 10);
        let output: Vec<f32> = skipper.collect();

        assert_eq!(output, vec![0.5, 0.6, 0.7]);
    }

    #[test]
    fn test_skip_silence_passes_through_audible_content() {
        let input = vec![0.3, 0.4, 0.5, 0.0001, 0.6, 0.7];
        let source = TestSampleSource::new(input.clone(), 1, 44100);
        let skipper = SkipSilence::with_frame_count(source, 0.002, 10);
        let output: Vec<f32> = skipper.collect();

        assert_eq!(output, input);
    }

    #[test]
    fn test_skip_silence_skips_sustained_trailing_silence() {
        let input = vec![0.5, 0.6, 0.0001, 0.0, 0.0, 0.0];
        let source = TestSampleSource::new(input, 1, 44100);
        let skipper = SkipSilence::with_frame_count(source, 0.002, 3);
        let output: Vec<f32> = skipper.collect();

        assert_eq!(output, vec![0.5, 0.6]);
    }

    #[test]
    fn test_skip_silence_stereo_interleaved_channel_coherence() {
        // Stereo interleaved: [L, R]
        // Frame 1: [0.0, 0.0] (silent)
        // Frame 2: [0.5, 0.0] (L is audible -> frame audible)
        // Frame 3: [0.0, 0.8] (R is audible -> frame audible)
        // Frame 4: [0.0, 0.0] (silent)
        // Frame 5: [0.0, 0.0] (silent -> sustained trailing >= 2 frames)
        let input = vec![0.0, 0.0, 0.5, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0, 0.0];
        let source = TestSampleSource::new(input, 2, 44100);
        let skipper = SkipSilence::with_frame_count(source, 0.002, 2);
        let output: Vec<f32> = skipper.collect();

        assert_eq!(output, vec![0.5, 0.0, 0.0, 0.8]);
    }

    #[test]
    fn test_normalizer_converges_gain_toward_target_on_loud_synthetic_buffer() {
        // 0 dBFS input: constant 1.0, 100 samples
        let input = vec![1.0; 100];
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 10, 0.05, 0.01);

        assert_eq!(normalizer.current_gain(), 1.0);
        let _output: Vec<f32> = normalizer.by_ref().collect();

        assert!(
            normalizer.current_gain() < 0.5,
            "expected gain to attenuate significantly toward target, got {}",
            normalizer.current_gain()
        );
        let target = normalizer.target_gain();
        let expected_target = db_to_linear(-14.0);
        assert!((target - expected_target).abs() < 1e-3);
    }

    #[test]
    fn test_normalizer_leaves_near_target_buffer_near_unity() {
        let near_target_amp = db_to_linear(-14.0);
        let input = vec![near_target_amp; 100];
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 10, 0.05, 0.01);

        let _output: Vec<f32> = normalizer.by_ref().collect();

        assert!(
            (normalizer.current_gain() - 1.0).abs() < 0.05,
            "expected gain to remain near unity, got {}",
            normalizer.current_gain()
        );
    }

    #[test]
    fn test_normalizer_bounded_no_runaway_gain() {
        // Moderate quiet input (-30 dBFS): target boost would be +16 dB (factor 6.3)
        // Must be clamped to max_gain (3.0)
        let quiet_amp = db_to_linear(-30.0);
        let input = vec![quiet_amp; 100];
        let source = TestSampleSource::new(input, 1, 44100);
        let mut normalizer = LoudnessNormalizer::with_parameters(source, -14.0, 10, 0.05, 0.01);

        let _output: Vec<f32> = normalizer.by_ref().collect();
        assert!(normalizer.target_gain() <= 3.0);
        assert!(normalizer.current_gain() <= 3.0);

        // Sub-noise-gate silence (< -60 dBFS): should not trigger gain increase
        let silent_input = vec![1e-5; 50];
        let silent_source = TestSampleSource::new(silent_input, 1, 44100);
        let mut silent_norm =
            LoudnessNormalizer::with_parameters(silent_source, -14.0, 10, 0.05, 0.01);
        let _silent_out: Vec<f32> = silent_norm.by_ref().collect();
        assert_eq!(silent_norm.target_gain(), 1.0);
    }

    #[test]
    fn test_dsp_pipeline_order_chaining() {
        // Sequence: [0.0 (silent), 1.0 (loud), 1.0 (loud), 0.0, 0.0 (trailing)]
        let input = vec![0.0, 1.0, 1.0, 0.0, 0.0];
        let source = TestSampleSource::new(input, 1, 44100);
        let skip = SkipSilence::with_frame_count(source, 0.002, 2);
        let normalizer = LoudnessNormalizer::with_parameters(skip, -14.0, 2, 0.5, 0.01);

        let output: Vec<f32> = normalizer.collect();
        // Leading silence is trimmed (first sample corresponds to input 1.0)
        // Trailing silence is skipped
        assert_eq!(output.len(), 2);
    }
}
