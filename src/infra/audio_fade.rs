//! Sample-accurate gain ramps applied inside the source chain.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rodio::Sink;

/// Number of output samples a ramp covers for `duration`.
///
/// The ramp runs per output sample (interleaved channels included), so the gain curve is clocked
/// by the audio device instead of a timer thread.
pub fn ramp_samples(duration: Duration, sample_rate: u32, channels: u16) -> u32 {
    let samples = duration.as_secs_f64() * f64::from(sample_rate) * f64::from(channels.max(1));
    if !samples.is_finite() || samples <= 0.0 {
        return 0;
    }
    samples.min(f64::from(u32::MAX)) as u32
}

/// Shared fade command channel between the playback controller and a [`GainRamp`].
///
/// The controller publishes a target gain and a ramp length in samples; the ramp applies the
/// command on the audio thread, so a crossfade is sample-accurate regardless of scheduler load.
#[derive(Debug)]
pub struct FadeControl {
    target_bits: AtomicU32,
    ramp_samples: AtomicU32,
    generation: AtomicU32,
}

impl FadeControl {
    /// Creates a control holding `initial_gain` with no ramp in flight.
    pub fn new(initial_gain: f32) -> Self {
        Self {
            target_bits: AtomicU32::new(initial_gain.to_bits()),
            ramp_samples: AtomicU32::new(0),
            generation: AtomicU32::new(0),
        }
    }

    /// Ramps to `target` over `samples` output samples; `0` samples applies the value at once.
    pub fn ramp_to(&self, target: f32, samples: u32) {
        self.target_bits.store(target.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.ramp_samples.store(samples, Ordering::Relaxed);
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Applies `target` immediately.
    pub fn set_now(&self, target: f32) {
        self.ramp_to(target, 0);
    }

    /// Reads the current command: `(target, samples, generation)`.
    fn command(&self) -> (f32, u32, u32) {
        let generation = self.generation.load(Ordering::Acquire);
        let target = f32::from_bits(self.target_bits.load(Ordering::Relaxed));
        let samples = self.ramp_samples.load(Ordering::Relaxed);
        (target, samples, generation)
    }
}

/// Source wrapper that applies a rampable gain to every sample it forwards.
///
/// With no command in flight the gain is constant and the wrapper only multiplies, so a track
/// played without a crossfade is bit-identical apart from that multiply.
pub struct GainRamp<I> {
    input: I,
    control: Arc<FadeControl>,
    gain: f32,
    target: f32,
    step: f32,
    remaining: u32,
    seen_generation: u32,
}

impl<I: rodio::Source<Item = f32>> GainRamp<I> {
    /// Wraps `input`, starting from the control's initial gain.
    pub fn new(input: I, control: Arc<FadeControl>) -> Self {
        let (target, _, generation) = control.command();
        Self {
            input,
            control,
            gain: target,
            target,
            step: 0.0,
            remaining: 0,
            seen_generation: generation,
        }
    }

    fn reload_command(&mut self) {
        let (target, samples, generation) = self.control.command();
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        self.target = target;
        if samples == 0 {
            self.gain = target;
            self.step = 0.0;
            self.remaining = 0;
            return;
        }
        self.step = (target - self.gain) / samples as f32;
        self.remaining = samples;
    }

    fn advance_gain(&mut self) {
        if self.remaining == 0 {
            return;
        }
        self.gain += self.step;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.gain = self.target;
        }
    }
}

impl<I: rodio::Source<Item = f32>> Iterator for GainRamp<I> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.reload_command();
        self.advance_gain();
        let sample = self.input.next()?;
        Some(sample * self.gain)
    }
}

impl<I: rodio::Source<Item = f32>> rodio::Source for GainRamp<I> {
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

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.input.try_seek(pos)
    }
}

/// Stops `sink` after `duration`, releasing the decoder once the ramp window has passed.
///
/// This timer only frees resources: the audible curve is applied per sample by [`GainRamp`], so
/// scheduler jitter here cannot change what the listener hears.
pub fn release_after(sink: Arc<Sink>, duration: Duration) {
    if duration.is_zero() {
        sink.stop();
        return;
    }
    std::thread::Builder::new()
        .name("audio-fade-release".to_string())
        .spawn(move || {
            std::thread::sleep(duration);
            sink.stop();
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use rodio::Source;

    use super::*;

    struct ConstantSource {
        remaining: usize,
        channels: u16,
        sample_rate: u32,
        duration: Option<Duration>,
    }

    impl ConstantSource {
        fn new(count: usize) -> Self {
            Self {
                remaining: count,
                channels: 2,
                sample_rate: 44_100,
                duration: Some(Duration::from_secs(1)),
            }
        }
    }

    impl Iterator for ConstantSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            if self.remaining == 0 {
                return None;
            }
            self.remaining -= 1;
            Some(1.0)
        }
    }

    impl rodio::Source for ConstantSource {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.remaining)
        }

        fn channels(&self) -> u16 {
            self.channels
        }

        fn sample_rate(&self) -> u32 {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            self.duration
        }

        fn try_seek(&mut self, _pos: Duration) -> Result<(), rodio::source::SeekError> {
            Ok(())
        }
    }

    #[test]
    fn ramp_samples_matches_specifications() {
        assert_eq!(ramp_samples(Duration::from_secs(1), 44_100, 2), 88_200);
        assert_eq!(ramp_samples(Duration::ZERO, 44_100, 2), 0);
        assert_eq!(ramp_samples(Duration::from_secs(1_000_000_000), 44_100, 2), u32::MAX);
        assert_eq!(ramp_samples(Duration::from_secs(1), 44_100, 0), 44_100);
    }

    #[test]
    fn constant_source_passes_unchanged_before_command() {
        let source = ConstantSource::new(50);
        let control = Arc::new(FadeControl::new(1.0));
        let ramp = GainRamp::new(source, control);
        let samples: Vec<f32> = ramp.collect();
        assert_eq!(samples.len(), 50);
        for sample in samples {
            assert_eq!(sample, 1.0);
        }
    }

    #[test]
    fn ramp_to_yields_monotonic_decreasing_gain_and_exact_end() {
        let source = ConstantSource::new(100);
        let control = Arc::new(FadeControl::new(1.0));
        let mut ramp = GainRamp::new(source, Arc::clone(&control));
        control.ramp_to(0.0, 100);
        let samples: Vec<f32> = ramp.by_ref().collect();
        assert_eq!(samples.len(), 100);
        for window in samples.windows(2) {
            assert!(window[0] >= window[1], "gain ramp must be monotonic decreasing");
        }
        assert!((samples[49] - 0.5).abs() < 1e-3, "midpoint was {}", samples[49]);
        assert_eq!(samples[99], 0.0, "last sample must be exactly 0.0");
    }

    #[test]
    fn set_now_takes_effect_on_next_sample() {
        let source = ConstantSource::new(10);
        let control = Arc::new(FadeControl::new(1.0));
        let mut ramp = GainRamp::new(source, Arc::clone(&control));
        assert_eq!(ramp.next(), Some(1.0));
        control.set_now(0.25);
        assert_eq!(ramp.next(), Some(0.25));
    }

    #[test]
    fn second_ramp_to_mid_ramp_retargets_without_large_jump() {
        let source = ConstantSource::new(100);
        let control = Arc::new(FadeControl::new(1.0));
        let mut ramp = GainRamp::new(source, Arc::clone(&control));
        control.ramp_to(0.0, 100);
        let mut samples = Vec::new();
        for _ in 0..30 {
            if let Some(s) = ramp.next() {
                samples.push(s);
            }
        }
        control.ramp_to(1.0, 50);
        for _ in 0..50 {
            if let Some(s) = ramp.next() {
                samples.push(s);
            }
        }
        for window in samples.windows(2) {
            let diff = (window[1] - window[0]).abs();
            assert!(diff <= 0.025, "jump between samples was {diff}");
        }
    }

    #[test]
    fn gain_ramp_delegates_source_properties() {
        let source = ConstantSource::new(10);
        let control = Arc::new(FadeControl::new(1.0));
        let mut ramp = GainRamp::new(source, control);
        assert_eq!(ramp.channels(), 2);
        assert_eq!(ramp.sample_rate(), 44_100);
        assert_eq!(ramp.total_duration(), Some(Duration::from_secs(1)));
        assert_eq!(ramp.current_span_len(), Some(10));
        assert!(ramp.try_seek(Duration::from_millis(500)).is_ok());
    }

    #[test]
    fn release_after_zero_duration_stops_sink() {
        let stream = match rodio::OutputStreamBuilder::open_default_stream() {
            Ok(stream) => stream,
            Err(_) => return,
        };
        let sink = Arc::new(Sink::connect_new(stream.mixer()));
        release_after(Arc::clone(&sink), Duration::ZERO);
        assert!(sink.empty());
    }
}
