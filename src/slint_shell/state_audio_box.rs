//! Blanket `AudioBackend` forwarding for boxed backends.
//!
//! The composition root and the workers hold `Box<dyn AudioBackend + Send>`, so every port
//! method must forward explicitly; a missing forward silently falls back to the trait default,
//! which degrades playback transitions instead of failing loudly.

use crate::infra::{AudioBackend, AudioError};

impl<T: ?Sized + AudioBackend> AudioBackend for Box<T> {
    fn play(&mut self, id: &str) -> Result<(), AudioError> {
        (**self).play(id)
    }
    fn play_crossfade(
        &mut self,
        id: &str,
        duration: std::time::Duration,
    ) -> Result<(), AudioError> {
        (**self).play_crossfade(id, duration)
    }
    fn play_gapless(&mut self, id: &str) -> Result<(), AudioError> {
        (**self).play_gapless(id)
    }
    fn take_started_gapless(&mut self) -> Option<crate::domain::StartedGapless> {
        (**self).take_started_gapless()
    }
    #[cfg(test)]
    fn finish_gapless(&mut self) {
        (**self).finish_gapless();
    }
    fn pause(&mut self) {
        (**self).pause();
    }
    fn resume_play(&mut self) {
        (**self).resume_play();
    }
    fn seek(&mut self, pos: u64) {
        (**self).seek(pos);
    }
    fn set_volume(&mut self, v: u8) {
        (**self).set_volume(v);
    }
    fn volume(&self) -> u8 {
        (**self).volume()
    }
    fn position_secs(&self) -> u64 {
        (**self).position_secs()
    }
    fn is_playing(&self) -> bool {
        (**self).is_playing()
    }
    fn is_paused(&self) -> bool {
        (**self).is_paused()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
    fn duration_secs(&self) -> u64 {
        (**self).duration_secs()
    }
    fn last_requested(&self) -> Option<&str> {
        (**self).last_requested()
    }
    fn set_quality(&mut self, q: crate::infra::AudioQuality) {
        (**self).set_quality(q);
    }
    fn quality(&self) -> crate::infra::AudioQuality {
        (**self).quality()
    }
    fn set_skip_silence(&mut self, on: bool) {
        (**self).set_skip_silence(on);
    }
    fn set_loudness_normalization(&mut self, on: bool) {
        (**self).set_loudness_normalization(on);
    }
    fn set_speed(&mut self, s: f32) {
        (**self).set_speed(s);
    }
    fn speed(&self) -> f32 {
        (**self).speed()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::domain::StartedGapless;

    #[derive(Default)]
    struct RecordingAudio {
        crossfade_secs: Arc<AtomicU64>,
        gapless_id: Arc<Mutex<Option<String>>>,
    }

    impl AudioBackend for RecordingAudio {
        fn play(&mut self, _track_id: &str) -> Result<(), AudioError> {
            Ok(())
        }

        fn play_crossfade(
            &mut self,
            _track_id: &str,
            duration: std::time::Duration,
        ) -> Result<(), AudioError> {
            self.crossfade_secs.store(duration.as_secs(), Ordering::SeqCst);
            Ok(())
        }

        fn play_gapless(&mut self, track_id: &str) -> Result<(), AudioError> {
            *self.gapless_id.lock().unwrap() = Some(track_id.to_owned());
            Ok(())
        }

        fn take_started_gapless(&mut self) -> Option<StartedGapless> {
            self.gapless_id
                .lock()
                .unwrap()
                .take()
                .map(|track_id| StartedGapless { track_id, duration_secs: None })
        }

        fn pause(&mut self) {}

        fn resume_play(&mut self) {}

        fn seek(&mut self, _position_secs: u64) {}

        fn set_volume(&mut self, _volume: u8) {}

        fn volume(&self) -> u8 {
            100
        }

        fn position_secs(&self) -> u64 {
            0
        }

        fn is_playing(&self) -> bool {
            false
        }

        fn last_requested(&self) -> Option<&str> {
            None
        }
    }

    #[test]
    fn boxed_backend_forwards_crossfade_and_gapless() {
        let recorder = RecordingAudio::default();
        let crossfade = Arc::clone(&recorder.crossfade_secs);
        let gapless = Arc::clone(&recorder.gapless_id);
        let mut boxed: Box<dyn AudioBackend + Send> = Box::new(recorder);

        assert_eq!(boxed.play_crossfade("cf", std::time::Duration::from_secs(4)), Ok(()));
        assert_eq!(crossfade.load(Ordering::SeqCst), 4, "crossfade forwarded through the box");

        assert_eq!(boxed.play_gapless("next"), Ok(()));
        let started = boxed.take_started_gapless().expect("started track forwarded");
        assert_eq!(started.track_id, "next");
        assert_eq!(*gapless.lock().unwrap(), None, "slot consumed through the box");
    }
}
