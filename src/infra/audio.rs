//! Audio port and no-op stub (no sound yet).

use super::stream::AudioQuality;

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
    fn set_stream_cache_budget_mb(&mut self, _mb: u32) {}
    fn stream_cache_total_bytes(&self) -> Option<u64> {
        None
    }
}

/// Recording stub: captures intent without touching audio hardware.
#[derive(Debug)]
pub struct AudioStub {
    last_requested: Option<String>,
    playing: bool,
    volume: u8,
    position_secs: u64,
    skip_silence: bool,
    loudness_normalization: bool,
    stream_cache_budget_mb: u32,
}

impl Default for AudioStub {
    fn default() -> Self {
        Self {
            last_requested: None,
            playing: false,
            volume: 100,
            position_secs: 0,
            skip_silence: false,
            loudness_normalization: false,
            stream_cache_budget_mb: 512,
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

    pub fn stream_cache_budget_mb(&self) -> u32 {
        self.stream_cache_budget_mb
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

    fn set_stream_cache_budget_mb(&mut self, mb: u32) {
        self.stream_cache_budget_mb = mb;
    }

    fn stream_cache_total_bytes(&self) -> Option<u64> {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(audio.stream_cache_budget_mb(), 512);
        audio.set_stream_cache_budget_mb(256);
        assert_eq!(audio.stream_cache_budget_mb(), 256);
        assert_eq!(audio.stream_cache_total_bytes(), Some(0));
    }
}
