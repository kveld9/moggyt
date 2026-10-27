//! Audio port and no-op stub (no sound yet).

use super::stream::AudioQuality;

/// Clamps a playback speed factor to the valid `0.5..=2.0` range.
///
/// NaN, negative, zero, and infinite values fall back to 1.0.
/// Finite positive values below 0.5 clamp to 0.5; values above 2.0 clamp to 2.0.
pub fn clamp_speed(speed: f32) -> f32 {
    if !speed.is_finite() || speed <= 0.0 {
        return 1.0;
    }
    speed.clamp(0.5, 2.0)
}

/// Playback port: stream control and observable playback state.
pub trait AudioBackend {
    fn play(&mut self, track_id: &str);
    fn pause(&mut self);
    fn resume_play(&mut self);
    fn seek(&mut self, position_secs: u64);
    fn set_volume(&mut self, volume: u8);
    fn volume(&self) -> u8;
    fn position_secs(&self) -> u64;
    fn is_playing(&self) -> bool;
    fn last_requested(&self) -> Option<&str>;
    fn set_quality(&mut self, _q: AudioQuality) {}
    fn set_skip_silence(&mut self, _on: bool) {}
    fn set_loudness_normalization(&mut self, _on: bool) {}
    /// Sets playback speed (valid range: 0.5..=2.0).
    ///
    /// Non-finite or non-positive values fall back to 1.0; finite values outside
    /// the range are clamped. Pitch preservation is out of scope (rate-only resampling).
    fn set_speed(&mut self, _speed: f32) {}
    /// Returns current playback speed factor (defaults to 1.0).
    fn speed(&self) -> f32 {
        1.0
    }
}

/// Recording stub: captures intent without touching audio hardware.
#[derive(Debug)]
pub struct AudioStub {
    last_requested: Option<String>,
    playing: bool,
    volume: u8,
    speed: f32,
    position_secs: u64,
    skip_silence: bool,
    loudness_normalization: bool,
}

impl Default for AudioStub {
    fn default() -> Self {
        Self {
            last_requested: None,
            playing: false,
            volume: 100,
            speed: 1.0,
            position_secs: 0,
            skip_silence: false,
            loudness_normalization: false,
        }
    }
}

impl AudioStub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn skip_silence(&self) -> bool {
        self.skip_silence
    }

    pub fn loudness_normalization(&self) -> bool {
        self.loudness_normalization
    }
}

impl AudioBackend for AudioStub {
    fn play(&mut self, track_id: &str) {
        self.last_requested = Some(track_id.to_owned());
        self.playing = true;
        self.position_secs = 0;
    }

    fn pause(&mut self) {
        self.playing = false;
    }

    fn resume_play(&mut self) {
        if self.last_requested.is_some() {
            self.playing = true;
        }
    }

    fn seek(&mut self, position_secs: u64) {
        self.position_secs = position_secs;
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn position_secs(&self) -> u64 {
        self.position_secs
    }

    fn is_playing(&self) -> bool {
        self.playing
    }

    fn last_requested(&self) -> Option<&str> {
        self.last_requested.as_deref()
    }

    fn set_skip_silence(&mut self, on: bool) {
        self.skip_silence = on;
    }

    fn set_loudness_normalization(&mut self, on: bool) {
        self.loudness_normalization = on;
    }

    fn set_speed(&mut self, speed: f32) {
        self.speed = clamp_speed(speed);
    }

    fn speed(&self) -> f32 {
        self.speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_speed_validation_table() {
        assert_eq!(clamp_speed(f32::NAN), 1.0);
        assert_eq!(clamp_speed(f32::INFINITY), 1.0);
        assert_eq!(clamp_speed(f32::NEG_INFINITY), 1.0);
        assert_eq!(clamp_speed(0.0), 1.0);
        assert_eq!(clamp_speed(-0.0), 1.0);
        assert_eq!(clamp_speed(-1.0), 1.0);
        assert_eq!(clamp_speed(-0.5), 1.0);

        // Lower boundary and below
        assert_eq!(clamp_speed(0.2), 0.5);
        assert_eq!(clamp_speed(0.49), 0.5);
        assert_eq!(clamp_speed(0.5), 0.5);

        // Presets
        assert_eq!(clamp_speed(0.75), 0.75);
        assert_eq!(clamp_speed(1.0), 1.0);
        assert_eq!(clamp_speed(1.25), 1.25);
        assert_eq!(clamp_speed(1.5), 1.5);

        // Upper boundary and above
        assert_eq!(clamp_speed(2.0), 2.0);
        assert_eq!(clamp_speed(2.01), 2.0);
        assert_eq!(clamp_speed(5.0), 2.0);
    }

    #[test]
    fn audio_stub_speed_recording_and_clamping() {
        let mut audio = AudioStub::new();
        assert_eq!(audio.speed(), 1.0);

        audio.set_speed(1.25);
        assert_eq!(audio.speed(), 1.25);

        audio.set_speed(0.1);
        assert_eq!(audio.speed(), 0.5);

        audio.set_speed(3.0);
        assert_eq!(audio.speed(), 2.0);

        audio.set_speed(f32::NAN);
        assert_eq!(audio.speed(), 1.0);

        audio.set_speed(-0.5);
        assert_eq!(audio.speed(), 1.0);
    }

    #[test]
    fn audio_state_transitions_and_volume_clamp() {
        let mut audio = AudioStub::new();
        assert!(!audio.skip_silence());
        assert!(!audio.loudness_normalization());
        for (v, exp) in [(0, 0), (50, 50), (100, 100), (150, 100)] {
            audio.set_volume(v);
            assert_eq!(audio.volume(), exp);
        }
        audio.resume_play();
        assert!(
            !audio.is_playing() && audio.last_requested().is_none() && audio.position_secs() == 0
        );
        audio.play("s-1");
        audio.seek(42);
        audio.pause();
        assert!(!audio.is_playing() && audio.position_secs() == 42);
        audio.resume_play();
        assert!(audio.is_playing() && audio.last_requested() == Some("s-1"));
        audio.play("s-2");
        assert!(audio.is_playing() && audio.position_secs() == 0);
        audio.set_quality(AudioQuality::High);
        audio.set_skip_silence(true);
        assert!(audio.skip_silence());
        audio.set_loudness_normalization(true);
        assert!(audio.loudness_normalization());
    }
}
