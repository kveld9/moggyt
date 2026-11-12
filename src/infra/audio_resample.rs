//! High-quality audio resampling adapter using rubato FFT resampler.
//!
//! Replaces rodio's default linear interpolation resampler with an FFT-based
//! resampler ([`FftFixedIn`]) to avoid passband attenuation and distortion
//! when decoded stream sample rates differ from the output stream rate.

use std::collections::VecDeque;
use std::time::Duration;

use anyhow::Context;
use rodio::Source;
use rodio::source::SeekError;
use rubato::{FftFixedIn, Resampler};

/// Wraps an audio [`Source`] and resamples it to `target_rate` via rubato FFT.
///
/// When the inner source sample rate matches `target_rate` (or is zero),
/// this acts as a zero-allocation pass-through iterator.
pub struct ResamplingSource<S> {
    inner: S,
    target_rate: u32,
    channels: u16,
    resampler: Option<FftFixedIn<f32>>,
    accumulator: Vec<f32>,
    input_buffers: Vec<Vec<f32>>,
    output_buffers: Vec<Vec<f32>>,
    output_queue: VecDeque<f32>,
    chunk_frames: usize,
    finished: bool,
}

impl<S> ResamplingSource<S>
where
    S: Source<Item = f32> + Send,
{
    /// Wraps `inner` in an FFT resampler targeting `target_rate`.
    ///
    /// When `inner` already reports `target_rate` (or rate 0), resampling is
    /// bypassed with pure pass-through and zero buffer allocation.
    pub fn new(inner: S, target_rate: u32) -> anyhow::Result<Self> {
        let source_rate = inner.sample_rate();
        let channels = inner.channels();
        if source_rate == target_rate || source_rate == 0 || channels == 0 {
            return Ok(Self {
                inner,
                target_rate,
                channels,
                resampler: None,
                accumulator: Vec::new(),
                input_buffers: Vec::new(),
                output_buffers: Vec::new(),
                output_queue: VecDeque::new(),
                chunk_frames: 0,
                finished: false,
            });
        }

        let resampler = FftFixedIn::<f32>::new(
            source_rate as usize,
            target_rate as usize,
            1024,
            2,
            channels as usize,
        )
        .context("failed to initialize FFT resampler")?;

        let chunk_frames = resampler.input_frames_next();
        let max_out_frames = resampler.output_frames_max();
        let ch_count = channels as usize;

        let accumulator = Vec::with_capacity(chunk_frames * ch_count);
        let input_buffers = vec![vec![0.0_f32; chunk_frames]; ch_count];
        let output_buffers = vec![vec![0.0_f32; max_out_frames]; ch_count];
        let output_queue = VecDeque::with_capacity(max_out_frames * ch_count);

        Ok(Self {
            inner,
            target_rate,
            channels,
            resampler: Some(resampler),
            accumulator,
            input_buffers,
            output_buffers,
            output_queue,
            chunk_frames,
            finished: false,
        })
    }

    fn deinterleave(&mut self, frames: usize) {
        let ch_count = self.channels as usize;
        for (ch, buf) in self.input_buffers.iter_mut().enumerate() {
            for (frame, sample) in buf.iter_mut().take(frames).enumerate() {
                *sample = self.accumulator[frame * ch_count + ch];
            }
            buf[frames..self.chunk_frames].fill(0.0);
        }
    }

    fn interleave(&mut self, produced_frames: usize) {
        let ch_count = self.channels as usize;
        for frame in 0..produced_frames {
            for ch in 0..ch_count {
                self.output_queue.push_back(self.output_buffers[ch][frame]);
            }
        }
    }

    fn process_full_chunk(&mut self) -> bool {
        self.deinterleave(self.chunk_frames);
        self.accumulator.clear();
        let Some(resampler) = &mut self.resampler else {
            return false;
        };
        match resampler.process_into_buffer(&self.input_buffers, &mut self.output_buffers, None) {
            Ok((_in_frames, out_frames)) => {
                self.interleave(out_frames);
                true
            }
            Err(err) => {
                tracing::warn!(error = ?err, "failed to resample audio chunk");
                self.finished = true;
                false
            }
        }
    }

    fn process_final_chunk(&mut self, collected_samples: usize) {
        let ch_count = self.channels as usize;
        let frames = collected_samples.div_ceil(ch_count);
        self.accumulator.resize(frames * ch_count, 0.0);
        self.deinterleave(frames);
        self.accumulator.clear();
        self.finished = true;

        let Some(resampler) = &mut self.resampler else {
            return;
        };
        match resampler.process_partial_into_buffer(
            Some(&self.input_buffers),
            &mut self.output_buffers,
            None,
        ) {
            Ok((_in_frames, out_frames)) => {
                self.interleave(out_frames);
            }
            Err(err) => {
                tracing::warn!(error = ?err, "failed to resample final audio chunk");
            }
        }
    }

    fn fill_and_process_next_chunk(&mut self) -> bool {
        let target_samples = self.chunk_frames * self.channels as usize;
        while self.accumulator.len() < target_samples {
            match self.inner.next() {
                Some(sample) => self.accumulator.push(sample),
                None => {
                    let collected = self.accumulator.len();
                    if collected > 0 {
                        self.process_final_chunk(collected);
                    } else {
                        self.finished = true;
                    }
                    return false;
                }
            }
        }
        self.process_full_chunk()
    }
}

impl<S> Iterator for ResamplingSource<S>
where
    S: Source<Item = f32> + Send,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.resampler.is_none() {
            return self.inner.next();
        }

        loop {
            if let Some(sample) = self.output_queue.pop_front() {
                return Some(sample);
            }
            if self.finished {
                return None;
            }
            if !self.fill_and_process_next_chunk() {
                return self.output_queue.pop_front();
            }
        }
    }
}

impl<S> Source for ResamplingSource<S>
where
    S: Source<Item = f32> + Send,
{
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.target_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let result = self.inner.try_seek(pos);
        if result.is_ok() {
            if let Some(resampler) = &mut self.resampler {
                resampler.reset();
            }
            self.accumulator.clear();
            self.output_queue.clear();
            self.finished = false;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestSource {
        samples: Vec<f32>,
        pos: usize,
        sample_rate: u32,
        channels: u16,
        duration: Option<Duration>,
    }

    impl TestSource {
        fn new(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Self {
            let total_frames = if channels > 0 { samples.len() / (channels as usize) } else { 0 };
            let duration = if sample_rate > 0 {
                Some(Duration::from_secs_f64(total_frames as f64 / sample_rate as f64))
            } else {
                None
            };
            Self { samples, pos: 0, sample_rate, channels, duration }
        }
    }

    impl Iterator for TestSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            if self.pos < self.samples.len() {
                let sample = self.samples[self.pos];
                self.pos += 1;
                Some(sample)
            } else {
                None
            }
        }
    }

    impl Source for TestSource {
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
            self.duration
        }

        fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
            let frame = (pos.as_secs_f64() * self.sample_rate as f64) as usize;
            let sample_idx = frame * self.channels as usize;
            self.pos = sample_idx.min(self.samples.len());
            Ok(())
        }
    }

    #[test]
    fn passthrough_when_rates_match() {
        let sample_rate = 48000;
        let target_rate = 48000;
        let samples = vec![0.1_f32, -0.2, 0.3, -0.4, 0.5, -0.6];
        let inner = TestSource::new(samples.clone(), sample_rate, 2);
        let mut resampler = ResamplingSource::new(inner, target_rate).expect("construct resampler");

        assert_eq!(resampler.sample_rate(), target_rate);
        assert_eq!(resampler.channels(), 2);
        let output: Vec<f32> = resampler.by_ref().collect();
        assert_eq!(output, samples);
    }

    #[test]
    fn upsamples_44100_to_48000_without_dropping_the_passband() {
        let sample_rate = 44100;
        let target_rate = 48000;
        let num_samples = 22050; // 0.5 s at 44.1 kHz
        let freq = 10000.0_f32;
        let input_samples: Vec<f32> = (0..num_samples)
            .map(|i| (2.0 * std::f32::consts::PI * freq * (i as f32) / (sample_rate as f32)).sin())
            .collect();

        let in_rms =
            (input_samples.iter().map(|&s| s * s).sum::<f32>() / input_samples.len() as f32).sqrt();

        let inner = TestSource::new(input_samples, sample_rate, 1);
        let mut resampler = ResamplingSource::new(inner, target_rate).expect("construct resampler");
        assert_eq!(resampler.sample_rate(), target_rate);

        let output_samples: Vec<f32> = resampler.by_ref().collect();

        // Discard 4096 samples at both ends to skip the resampler delay
        assert!(output_samples.len() > 8192, "output too short to trim");
        let trimmed = &output_samples[4096..output_samples.len() - 4096];
        let out_rms = (trimmed.iter().map(|&s| s * s).sum::<f32>() / trimmed.len() as f32).sqrt();

        let ratio_db = 20.0 * (out_rms / in_rms).log10();
        assert!(
            ratio_db.abs() < 0.2,
            "RMS ratio {ratio_db} dB outside 0.2 dB tolerance (in_rms={in_rms}, out_rms={out_rms})"
        );

        let ratio = target_rate as f64 / sample_rate as f64;
        let expected_len = (num_samples as f64 * ratio) as usize;
        let chunk_tolerance = 2 * 1024 * target_rate as usize / sample_rate as usize;
        let len_diff = (output_samples.len() as isize - expected_len as isize).abs();
        assert!(
            len_diff <= chunk_tolerance as isize,
            "output length {} differs from expected {} by {} (tolerance {})",
            output_samples.len(),
            expected_len,
            len_diff,
            chunk_tolerance
        );
    }

    #[test]
    fn stereo_interleaving_is_preserved() {
        let sample_rate = 44100;
        let target_rate = 48000;
        let frame_count = 5000;
        let mut samples = Vec::with_capacity(frame_count * 2);
        for _ in 0..frame_count {
            samples.push(0.5_f32);
            samples.push(-0.5_f32);
        }

        let inner = TestSource::new(samples, sample_rate, 2);
        let mut resampler = ResamplingSource::new(inner, target_rate).expect("construct resampler");
        let output: Vec<f32> = resampler.by_ref().collect();

        // After discarding the first output chunk (e.g. 2048 interleaved samples = 1024 frames)
        let discard_samples = 2048;
        assert!(output.len() > discard_samples * 2);
        let trimmed = &output[discard_samples..output.len() - discard_samples];
        for (i, &sample) in trimmed.iter().enumerate() {
            if i % 2 == 0 {
                assert!(
                    (sample - 0.5).abs() < 0.02,
                    "even sample {i} was {sample}, expected approx +0.5"
                );
            } else {
                assert!(
                    (sample - (-0.5)).abs() < 0.02,
                    "odd sample {i} was {sample}, expected approx -0.5"
                );
            }
        }
    }

    #[test]
    fn try_seek_resets_state_and_keeps_producing_audio() {
        let sample_rate = 44100;
        let target_rate = 48000;
        let num_samples = 44100; // 1.0 s of mono audio
        let samples: Vec<f32> = (0..num_samples).map(|i| (i as f32) / 44100.0).collect();

        let inner = TestSource::new(samples, sample_rate, 1);
        let mut resampler = ResamplingSource::new(inner, target_rate).expect("construct resampler");

        for _ in 0..500 {
            assert!(resampler.next().is_some());
        }

        assert!(resampler.try_seek(Duration::from_millis(100)).is_ok());

        for _ in 0..500 {
            assert!(resampler.next().is_some());
        }
    }

    #[test]
    fn final_partial_chunk_does_not_get_lost() {
        let sample_rate = 44100;
        let target_rate = 48000;
        let num_samples = 1500; // not a multiple of 1024
        let samples: Vec<f32> = (0..num_samples).map(|i| ((i % 100) as f32) / 100.0).collect();

        let inner = TestSource::new(samples, sample_rate, 1);
        let mut resampler = ResamplingSource::new(inner, target_rate).expect("construct resampler");
        let output: Vec<f32> = resampler.by_ref().collect();

        let ratio = target_rate as f64 / sample_rate as f64;
        let expected_len = (num_samples as f64 * ratio) as usize;
        let chunk_tolerance = 2 * 1024 * target_rate as usize / sample_rate as usize;
        let diff = (output.len() as isize - expected_len as isize).abs();
        assert!(
            diff <= chunk_tolerance as isize,
            "output len {} differs from expected {} by {} (tolerance {})",
            output.len(),
            expected_len,
            diff,
            chunk_tolerance
        );
        assert!(!output.is_empty(), "output must not be empty");
    }
}
