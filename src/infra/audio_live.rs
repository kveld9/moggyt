//! Dual-mode audio backend providing hardware playback with fallback to an in-memory stub.

use std::sync::Arc;
use std::time::Duration;

use super::audio::{AudioBackend, AudioError, AudioStub};
use super::audio_rodio::RodioBackend;
use super::stream::AudioQuality;
use super::stream_chain::StreamChain;
use super::stream_disk_cache::StreamDiskCache;

/// Dual-mode audio backend: hardware playback with graceful fallback to an in-memory stub.
pub enum LiveAudio {
    /// Live rodio backend connected to audio hardware.
    Live(RodioBackend),
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

    /// Configures or clears the backing stream disk cache on live hardware.
    pub fn set_stream_cache(&mut self, cache: Option<Arc<StreamDiskCache>>) {
        if let Self::Live(backend) = self {
            backend.set_stream_cache(cache);
        }
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

    fn is_playing(&self) -> bool {
        delegate_audio!(self, is_playing)
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
        assert!(audio.stream_prefetcher().is_none());
        assert!(!audio.has_cached_stream("track-muted-1"));
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
}
