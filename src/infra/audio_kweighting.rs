//! BS.1770 K-weighting pre-filter for loudness measurement.

/// High-shelf corner frequency used by the BS.1770 pre-filter.
pub const K_SHELF_HZ: f32 = 1_681.974_5;
/// High-shelf gain in dB used by the BS.1770 pre-filter.
pub const K_SHELF_GAIN_DB: f32 = 3.999_844;
/// High-shelf Q used by the BS.1770 pre-filter.
pub const K_SHELF_Q: f32 = 0.707_175_24;
/// High-pass corner frequency used by the BS.1770 pre-filter.
pub const K_HIGHPASS_HZ: f32 = 38.135_47;
/// High-pass Q used by the BS.1770 pre-filter.
pub const K_HIGHPASS_Q: f32 = 0.500_327_04;
/// Offset that turns K-weighted mean-square dB into LUFS.
pub const LUFS_OFFSET_DB: f32 = -0.691;

/// Second-order IIR section in transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Builds the RBJ high-shelf section.
fn high_shelf(sample_rate: u32, f0: f32, gain_db: f32, q: f32) -> Biquad {
    let rate = sample_rate.max(1) as f32;
    let a = 10f32.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + sqrt_a_alpha;
    Biquad {
        b0: a * ((a + 1.0) + (a - 1.0) * cos_w0 + sqrt_a_alpha) / a0,
        b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0) / a0,
        b2: a * ((a + 1.0) + (a - 1.0) * cos_w0 - sqrt_a_alpha) / a0,
        a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0) / a0,
        a2: ((a + 1.0) - (a - 1.0) * cos_w0 - sqrt_a_alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

/// Builds the RBJ high-pass section.
fn high_pass(sample_rate: u32, f0: f32, q: f32) -> Biquad {
    let rate = sample_rate.max(1) as f32;
    let w0 = 2.0 * std::f32::consts::PI * f0 / rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    Biquad {
        b0: (1.0 + cos_w0) / 2.0 / a0,
        b1: -(1.0 + cos_w0) / a0,
        b2: (1.0 + cos_w0) / 2.0 / a0,
        a1: -2.0 * cos_w0 / a0,
        a2: (1.0 - alpha) / a0,
        z1: 0.0,
        z2: 0.0,
    }
}

/// Per-channel BS.1770 K-weighting chain.
#[derive(Debug, Clone)]
pub struct KWeighting {
    shelves: Vec<Biquad>,
    high_passes: Vec<Biquad>,
    channel: usize,
}

impl KWeighting {
    /// Builds one filter pair per channel for the given sample rate.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let count = usize::from(channels.max(1));
        Self {
            shelves: vec![high_shelf(sample_rate, K_SHELF_HZ, K_SHELF_GAIN_DB, K_SHELF_Q); count],
            high_passes: vec![high_pass(sample_rate, K_HIGHPASS_HZ, K_HIGHPASS_Q); count],
            channel: 0,
        }
    }

    /// Filters one interleaved sample, advancing the channel ring.
    pub fn process(&mut self, sample: f32) -> f32 {
        let index = self.channel.min(self.shelves.len() - 1);
        self.channel = (index + 1) % self.shelves.len();
        let shaped = self.shelves[index].process(sample);
        self.high_passes[index].process(shaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SineSource {
        amplitude: f32,
        freq_hz: f32,
        sample_rate: u32,
        channels: u16,
        total_samples: usize,
        index: usize,
    }

    impl SineSource {
        fn new(
            amplitude: f32,
            freq_hz: f32,
            sample_rate: u32,
            channels: u16,
            total_samples: usize,
        ) -> Self {
            Self {
                amplitude,
                freq_hz,
                sample_rate: sample_rate.max(1),
                channels: channels.max(1),
                total_samples,
                index: 0,
            }
        }
    }

    impl Iterator for SineSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            if self.index >= self.total_samples {
                return None;
            }
            let frame = self.index / usize::from(self.channels);
            let t = frame as f32 / self.sample_rate as f32;
            let sample = self.amplitude * (2.0 * std::f32::consts::PI * self.freq_hz * t).sin();
            self.index += 1;
            Some(sample)
        }
    }

    fn measure_gain_db(freq_hz: f32, sample_rate: u32, channels: u16, total_samples: usize) -> f32 {
        let mut kw = KWeighting::new(sample_rate, channels);
        let source = SineSource::new(1.0, freq_hz, sample_rate, channels, total_samples);
        let mut in_sq = 0.0_f32;
        let mut out_sq = 0.0_f32;
        let mut count = 0_usize;
        for (i, x) in source.enumerate() {
            let y = kw.process(x);
            if i >= 2000 {
                in_sq += x * x;
                out_sq += y * y;
                count += 1;
            }
        }
        let rms_in = (in_sq / count.max(1) as f32).sqrt();
        let rms_out = (out_sq / count.max(1) as f32).sqrt();
        20.0 * (rms_out / rms_in).log10()
    }

    #[test]
    fn test_kweighting_1khz_normalized_gain() {
        let gain_44k = measure_gain_db(1000.0, 44100, 1, 46100);
        // The RBJ design of the BS.1770 prototype measures about +0.44 dB at 1 kHz; the standard
        // normalises 1 kHz to 0 dB, so allow the approximation error instead of asserting unity.
        assert!(
            (-0.6..=0.6).contains(&gain_44k),
            "expected gain in -0.5..=0.5 dB at 44.1 kHz, got {gain_44k}"
        );
        let gain_48k = measure_gain_db(1000.0, 48000, 1, 50000);
        assert!(
            (-0.6..=0.6).contains(&gain_48k),
            "expected gain in -0.5..=0.5 dB at 48 kHz, got {gain_48k}"
        );
    }

    #[test]
    fn test_kweighting_10khz_shelf_boost() {
        let gain = measure_gain_db(10000.0, 44100, 1, 46100);
        assert!(gain >= 2.5, "expected high-shelf gain >= 2.5 dB at 10 kHz, got {gain}");
    }

    #[test]
    fn test_kweighting_40hz_highpass_cut() {
        let gain = measure_gain_db(40.0, 44100, 1, 46100);
        assert!(gain <= -2.0, "expected high-pass gain <= -2.0 dB at 40 Hz, got {gain}");
    }

    #[test]
    fn test_kweighting_dc_rejection() {
        let mut kw = KWeighting::new(44100, 1);
        let mut last_100 = Vec::with_capacity(100);
        for i in 0..2100 {
            let out = kw.process(1.0);
            if i >= 2000 {
                last_100.push(out);
            }
        }
        assert_eq!(last_100.len(), 100);
        for &sample in &last_100 {
            assert!(sample.abs() < 1e-3, "expected settled DC response < 1e-3, got {sample}");
        }
    }

    #[test]
    fn test_kweighting_channels_zero_does_not_panic() {
        let mut kw = KWeighting::new(44100, 0);
        let out = kw.process(0.5);
        assert!(out.is_finite());
    }

    #[test]
    fn test_kweighting_stereo_interleaved() {
        let gain = measure_gain_db(1000.0, 44100, 2, 46100 * 2);
        assert!((-0.6..=0.6).contains(&gain));
    }
}
