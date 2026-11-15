//! Dual-mode audio backend providing hardware playback with fallback to an in-memory stub.

use std::time::Duration;

use super::audio::{AudioBackend, AudioError, AudioStub};
use super::stream::AudioQuality;
use super::stream_chain::StreamChain;

/// Dual-mode audio backend: hardware playback with graceful fallback to an in-memory stub.
pub enum LiveAudio {
    /// Live rodio backend connected to audio hardware.
    Live(Box<dyn AudioBackend + Send>),
    /// Muted fallback storing state in memory when hardware or dependencies are unavailable.
    Muted(AudioStub),
}

impl LiveAudio {
    /// Creates a muted backend variant.
    pub fn muted() -> Self {
        Self::Muted(AudioStub::new())
    }

    /// Returns whether this backend is actively backed by audio hardware.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live(_))
    }
}

macro_rules! delegate_audio {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Self::Live(backend) => backend.$method($($arg),*),
            Self::Muted(stub) => stub.$method($($arg),*),
        }
    };
}

impl AudioBackend for LiveAudio {
    fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
        match self {
            Self::Live(backend) => backend.play(track_id),
            Self::Muted(_) => Err(AudioError::Unavailable),
        }
    }

    fn play_crossfade(&mut self, track_id: &str, duration: Duration) -> Result<(), AudioError> {
        match self {
            Self::Live(backend) => backend.play_crossfade(track_id, duration),
            Self::Muted(_) => Err(AudioError::Unavailable),
        }
    }

    fn play_gapless(&mut self, track_id: &str) -> Result<(), AudioError> {
        match self {
            Self::Live(backend) => backend.play_gapless(track_id),
            Self::Muted(_) => Err(AudioError::Unavailable),
        }
    }

    fn take_started_gapless(&mut self) -> Option<crate::domain::StartedGapless> {
        delegate_audio!(self, take_started_gapless)
    }

    fn pause(&mut self) {
        delegate_audio!(self, pause);
    }

    fn resume_play(&mut self) {
        delegate_audio!(self, resume_play);
    }

    fn seek(&mut self, position_secs: u64) {
        delegate_audio!(self, seek, position_secs);
    }

    fn set_volume(&mut self, volume: u8) {
        delegate_audio!(self, set_volume, volume);
    }

    fn volume(&self) -> u8 {
        delegate_audio!(self, volume)
    }

    fn set_speed(&mut self, speed: f32) {
        delegate_audio!(self, set_speed, speed);
    }

    fn speed(&self) -> f32 {
        delegate_audio!(self, speed)
    }

    fn position_secs(&self) -> u64 {
        delegate_audio!(self, position_secs)
    }

    fn duration_secs(&self) -> u64 {
        delegate_audio!(self, duration_secs)
    }

    fn is_playing(&self) -> bool {
        delegate_audio!(self, is_playing)
    }

    fn is_paused(&self) -> bool {
        delegate_audio!(self, is_paused)
    }

    fn is_empty(&self) -> bool {
        delegate_audio!(self, is_empty)
    }

    fn last_requested(&self) -> Option<&str> {
        delegate_audio!(self, last_requested)
    }

    fn set_quality(&mut self, quality: AudioQuality) {
        delegate_audio!(self, set_quality, quality);
    }

    fn quality(&self) -> AudioQuality {
        delegate_audio!(self, quality)
    }

    fn set_skip_silence(&mut self, on: bool) {
        delegate_audio!(self, set_skip_silence, on);
    }

    fn set_loudness_normalization(&mut self, on: bool) {
        delegate_audio!(self, set_loudness_normalization, on);
    }

    fn stream_prefetcher(&self) -> Option<StreamChain> {
        delegate_audio!(self, stream_prefetcher)
    }

    fn has_cached_stream(&self, track_id: &str) -> bool {
        delegate_audio!(self, has_cached_stream, track_id)
    }

    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        delegate_audio!(self, set_session_cookies, cookies);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_audio_muted_play_reports_unavailable() {
        let mut audio = LiveAudio::muted();
        assert!(!audio.is_live());
        assert_eq!(audio.last_requested(), None);
        assert!(!audio.is_playing());
        assert_eq!(audio.position_secs(), 0);
        assert_eq!(audio.volume(), 100);

        let res = audio.play("track-stub-1");
        assert_eq!(res, Err(AudioError::Unavailable));
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);

        audio.seek(42);
        assert_eq!(audio.position_secs(), 42);

        audio.set_volume(80);
        assert_eq!(audio.volume(), 80);

        audio.set_volume(150);
        assert_eq!(audio.volume(), 100);

        assert_eq!(audio.speed(), 1.0);
        audio.set_speed(1.25);
        assert_eq!(audio.speed(), 1.25);
        audio.set_speed(3.0);
        assert_eq!(audio.speed(), 2.0);
        audio.set_speed(0.1);
        assert_eq!(audio.speed(), 0.5);
        audio.set_speed(f32::NAN);
        assert_eq!(audio.speed(), 1.0);

        assert_eq!(audio.quality(), AudioQuality::Standard);
        audio.set_quality(AudioQuality::High);
        assert_eq!(audio.quality(), AudioQuality::High);
        audio.set_quality(AudioQuality::Compact);
        assert_eq!(audio.quality(), AudioQuality::Compact);
        audio.set_skip_silence(true);
        audio.set_loudness_normalization(true);
        audio.set_session_cookies(Some("SAPISID=live_test"));
        assert!(audio.stream_prefetcher().is_none());
        assert!(!audio.has_cached_stream("track-muted-1"));
        audio.set_session_cookies(None);
    }

    #[test]
    fn live_audio_muted_play_crossfade_reports_unavailable() {
        let mut audio = LiveAudio::muted();
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);

        let res = audio.play_crossfade("track-cf-muted", Duration::from_secs(4));
        assert_eq!(res, Err(AudioError::Unavailable));
        assert!(!audio.is_playing());
        assert_eq!(audio.last_requested(), None);
    }

    struct ReportedAudio {
        stub: AudioStub,
        playing: bool,
        paused: bool,
        empty: bool,
    }

    impl AudioBackend for ReportedAudio {
        fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
            self.stub.play(track_id)
        }

        fn pause(&mut self) {
            self.stub.pause();
        }

        fn resume_play(&mut self) {
            self.stub.resume_play();
        }

        fn seek(&mut self, position_secs: u64) {
            self.stub.seek(position_secs);
        }

        fn set_volume(&mut self, volume: u8) {
            self.stub.set_volume(volume);
        }

        fn volume(&self) -> u8 {
            self.stub.volume()
        }

        fn position_secs(&self) -> u64 {
            self.stub.position_secs()
        }

        fn is_playing(&self) -> bool {
            self.playing
        }

        fn is_paused(&self) -> bool {
            self.paused
        }

        fn is_empty(&self) -> bool {
            self.empty
        }

        fn last_requested(&self) -> Option<&str> {
            self.stub.last_requested()
        }

        fn duration_secs(&self) -> u64 {
            213
        }
    }

    fn reported_audio(paused: bool, empty: bool) -> ReportedAudio {
        ReportedAudio { stub: AudioStub::new(), playing: false, paused, empty }
    }

    #[test]
    fn live_audio_forwards_the_reported_track_duration() {
        let live = LiveAudio::Live(Box::new(reported_audio(false, false)));
        assert_eq!(live.duration_secs(), 213);
        assert_eq!(LiveAudio::muted().duration_secs(), 0);
    }

    #[test]
    fn live_audio_forwards_pause_and_drained_sink_facts() {
        let paused = LiveAudio::Live(Box::new(reported_audio(true, false)));
        assert!(paused.is_paused());
        assert!(!paused.is_empty());

        let drained = LiveAudio::Live(Box::new(reported_audio(false, true)));
        assert!(!drained.is_paused());
        assert!(drained.is_empty());
    }

    #[test]
    fn drained_live_backend_maps_to_the_ended_event() {
        use crate::domain::{PlaybackEvent, PlaybackFacts};

        let mut stub = AudioStub::new();
        stub.play("track-ended").expect("stub play");
        let live = LiveAudio::Live(Box::new(ReportedAudio {
            stub,
            playing: false,
            paused: false,
            empty: true,
        }));
        let facts = PlaybackFacts {
            has_track: true,
            is_empty: live.is_empty(),
            is_paused: live.is_paused(),
            is_playing: live.is_playing(),
            position_secs: live.position_secs(),
            duration_secs: live.duration_secs(),
        };
        assert_eq!(facts.event(), Some(PlaybackEvent::Ended));
    }
}
