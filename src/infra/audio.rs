//! Audio port and no-op stub (no sound yet).

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
}

/// Recording stub: captures intent without touching audio hardware.
#[derive(Debug)]
pub struct AudioStub {
    last_requested: Option<String>,
    playing: bool,
    volume: u8,
    position_secs: u64,
}

impl Default for AudioStub {
    fn default() -> Self {
        Self { last_requested: None, playing: false, volume: 100, position_secs: 0 }
    }
}

impl AudioStub {
    pub fn new() -> Self {
        Self::default()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_state_transitions_and_volume_clamp() {
        let mut audio = AudioStub::new();
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
    }
}
