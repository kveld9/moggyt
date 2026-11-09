//! Audio port and no-op stub (no sound yet).

use super::stream::AudioQuality;
use super::ytdlp::YtDlpResolver;

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

/// Failure of a playback request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// The backend cannot produce audio (muted, unavailable device, unconfigured).
    Unavailable,
    /// The stream could not be resolved or decoded.
    Playback(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "audio backend unavailable"),
            Self::Playback(msg) => write!(f, "playback failed: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// Playback port: stream control and observable playback state.
pub trait AudioBackend {
    /// Begins audible playback of a track.
    ///
    /// Success means the backend actually started audible playback of that track.
    fn play(&mut self, track_id: &str) -> Result<(), AudioError>;
    /// Begins playback of a track with crossfade transition from current audio.
    ///
    /// Success means the backend actually started audible playback of that track.
    /// Implementations that do not support crossfading fall back to immediate playback via [`play`](Self::play).
    fn play_crossfade(
        &mut self,
        track_id: &str,
        _duration: std::time::Duration,
    ) -> Result<(), AudioError> {
        self.play(track_id)
    }
    fn pause(&mut self);
    fn resume_play(&mut self);
    fn seek(&mut self, position_secs: u64);
    fn set_volume(&mut self, volume: u8);
    fn volume(&self) -> u8;
    fn position_secs(&self) -> u64;
    fn is_playing(&self) -> bool;
    /// Returns whether playback is currently paused. Exposes raw facts only; adapters must never derive the phase.
    fn is_paused(&self) -> bool {
        self.last_requested().is_some() && !self.is_playing()
    }
    /// Returns whether the backend has no buffered audio left for the loaded track (a drained sink); it is not the phase.
    fn is_empty(&self) -> bool {
        self.last_requested().is_none()
    }
    /// Returns track duration in seconds (0 means unknown). Exposes raw facts only; adapters must never derive the phase.
    fn duration_secs(&self) -> u64 {
        0
    }
    fn last_requested(&self) -> Option<&str>;
    fn set_quality(&mut self, _q: AudioQuality) {}
    /// Returns current audio streaming quality preference (defaults to [`AudioQuality::Standard`]).
    fn quality(&self) -> AudioQuality {
        AudioQuality::default()
    }
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
    /// Returns a resolver handle sharing this backend's stream URL cache, for off-lock prefetching.
    fn stream_prefetcher(&self) -> Option<YtDlpResolver> {
        None
    }
}

/// In-memory no-op audio stub alias.
pub type NullAudio = AudioStub;

/// Recording stub: captures intent without touching audio hardware.
#[derive(Debug)]
pub struct AudioStub {
    fail_plays: bool,
    last_requested: Option<String>,
    playing: bool,
    volume: u8,
    speed: f32,
    position_secs: u64,
    quality: AudioQuality,
    skip_silence: bool,
    loudness_normalization: bool,
}

impl Default for AudioStub {
    fn default() -> Self {
        Self {
            fail_plays: false,
            last_requested: None,
            playing: false,
            volume: 100,
            speed: 1.0,
            position_secs: 0,
            quality: AudioQuality::default(),
            skip_silence: false,
            loudness_normalization: false,
        }
    }
}

impl AudioStub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an audio stub configured to fail playback attempts.
    pub fn failing() -> Self {
        Self { fail_plays: true, ..Self::default() }
    }

    /// Configures whether playback attempts should fail.
    pub fn set_fail_plays(&mut self, fail: bool) {
        self.fail_plays = fail;
    }

    pub fn skip_silence(&self) -> bool {
        self.skip_silence
    }

    pub fn loudness_normalization(&self) -> bool {
        self.loudness_normalization
    }

    /// Returns the last requested track identifier, if any.
    pub fn last_requested_id(&self) -> Option<&str> {
        self.last_requested.as_deref()
    }
}

impl AudioBackend for AudioStub {
    fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
        if self.fail_plays {
            return Err(AudioError::Unavailable);
        }
        self.last_requested = Some(track_id.to_owned());
        self.playing = true;
        self.position_secs = 0;
        Ok(())
    }

    fn play_crossfade(
        &mut self,
        track_id: &str,
        _duration: std::time::Duration,
    ) -> Result<(), AudioError> {
        self.play(track_id)
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

    fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
    }

    fn quality(&self) -> AudioQuality {
        self.quality
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
        assert_eq!(audio.play("s-1"), Ok(()));
        audio.seek(42);
        audio.pause();
        assert!(!audio.is_playing() && audio.position_secs() == 42);
        audio.resume_play();
        assert!(audio.is_playing() && audio.last_requested() == Some("s-1"));
        assert_eq!(audio.play("s-2"), Ok(()));
        assert!(audio.is_playing() && audio.position_secs() == 0);
        audio.set_quality(AudioQuality::High);
        audio.set_skip_silence(true);
        assert!(audio.skip_silence());
        audio.set_loudness_normalization(true);
        assert!(audio.loudness_normalization());
    }

    #[test]
    fn audio_stub_quality_recording() {
        let mut audio = AudioStub::new();
        assert_eq!(audio.quality(), AudioQuality::Standard);
        audio.set_quality(AudioQuality::High);
        assert_eq!(audio.quality(), AudioQuality::High);
    }

    #[test]
    fn audio_stub_stream_prefetcher_default_is_none() {
        let audio = AudioStub::new();
        assert!(audio.stream_prefetcher().is_none());
    }

    #[test]
    fn null_audio_play_crossfade_updates_state() {
        let mut audio = NullAudio::new();
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);
        assert_eq!(audio.last_requested_id(), None);

        assert_eq!(audio.play_crossfade("t-cross-1", std::time::Duration::from_secs(5)), Ok(()));
        assert!(audio.is_playing());
        assert_eq!(audio.last_requested(), Some("t-cross-1"));
        assert_eq!(audio.last_requested_id(), Some("t-cross-1"));
        assert_eq!(audio.position_secs(), 0);
    }

    #[test]
    fn audio_stub_failing_play_reports_error_without_state_change() {
        let mut audio = AudioStub::failing();
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);

        let res = audio.play("track-fail");
        assert_eq!(res, Err(AudioError::Unavailable));
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);
        assert_eq!(audio.position_secs(), 0);
    }

    #[test]
    fn audio_stub_play_reports_ok_and_marks_playing() {
        let mut audio = AudioStub::new();
        assert_eq!(audio.play("track-ok"), Ok(()));
        assert!(audio.is_playing());
        assert_eq!(audio.last_requested(), Some("track-ok"));
        assert_eq!(audio.position_secs(), 0);
    }
}
