//! Single playback transition request builder and dispatch authority.

use crate::domain::PlaybackPhase;

#[cfg(test)]
use crate::slint_shell::playback_worker::PlaybackWorkerAdapters;
use crate::slint_shell::playback_worker::{
    LyricsPrefetch, PlayTrackRequest, PlaybackWorkerCommand,
};
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Constructs a play request for the current track and bumps the playback generation.
    ///
    /// Runs the sleep-timer track-change hook for the target track before bumping the
    /// generation. The hook pauses audio and returns true for `StopAfterTrack`; returning
    /// `None` means no request is dispatched for that transition.
    ///
    /// Marks the phase as `Resolving` so a track that already reached `Ended` cannot advance
    /// the queue again before the new stream starts.
    pub fn begin_play_request(&mut self) -> Option<PlayTrackRequest> {
        let track = self.queue.current().cloned()?;
        if self.on_track_changing_sleep_timer(&track.id) {
            return None;
        }
        self.set_playback_phase(PlaybackPhase::Resolving);
        self.play_generation = self.play_generation.wrapping_add(1);
        let generation = self.play_generation;
        self.maybe_scrobble_current();

        let next_track = self.queue.peek_next().filter(|next| next.id != track.id);
        let prefetch_lyrics = next_track.as_ref().map(|next| LyricsPrefetch {
            artist: next.artist.clone(),
            title: next.title.clone(),
            duration_secs: next.duration_secs,
        });
        let prefetch_stream_id =
            next_track.map(|next| self.version_overrides.resolve(&next.id).to_owned());

        Some(PlayTrackRequest {
            generation,
            track_id: self.version_overrides.resolve(&track.id).to_owned(),
            title: track.title,
            artist: track.artist,
            artwork_url: track.artwork,
            duration_secs: track.duration_secs,
            hi_res_covers: self.hi_res_covers,
            crossfade_seconds: self.crossfade_seconds,
            lyric_overrides: self.lyric_overrides.clone(),
            prefetch_lyrics,
            prefetch_stream_id,
        })
    }

    /// Dispatches a play request to the background worker or test synchronous fallback.
    pub fn send_play_request(&mut self, req: PlayTrackRequest) {
        if let Some(ref tx) = self.worker_tx {
            let _ = tx.send(PlaybackWorkerCommand::Play(req));
        } else {
            self.dispatch_without_worker(req);
        }
    }

    /// Constructs and dispatches a play request for the current track cursor.
    pub fn request_current_play(&mut self) -> bool {
        let Some(req) = self.begin_play_request() else {
            return false;
        };
        self.send_play_request(req);
        true
    }

    #[cfg(test)]
    fn dispatch_without_worker(&mut self, req: PlayTrackRequest) {
        let adapters = PlaybackWorkerAdapters::new(
            std::sync::Arc::clone(&self.audio),
            std::sync::Arc::clone(&self.scrobbler),
            self.secondary_scrobbler.clone(),
            std::sync::Arc::clone(&self.lyrics),
            std::sync::Arc::clone(&self.covers),
        );
        let _ = crate::slint_shell::playback_worker::execute_play_track(req, &adapters);
    }

    #[cfg(not(test))]
    fn dispatch_without_worker(&mut self, _req: PlayTrackRequest) {}
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::domain::Track;
    use crate::slint_shell::test_support::{EndedAudio, test_wired};

    #[test]
    fn begin_play_request_bumps_generation_and_targets_current_track() {
        let mut state = test_wired();
        state.queue.push(Track::new("trk-1", "Track 1", "Artist 1"));
        state.queue.push(Track::new("trk-2", "Track 2", "Artist 2"));
        let initial_gen = state.play_generation;

        let req1 = state.begin_play_request().expect("first track request");
        assert_eq!(req1.generation, initial_gen.wrapping_add(1));
        assert_eq!(req1.track_id, "trk-1");

        state.queue.advance();
        let req2 = state.begin_play_request().expect("second track request");
        assert_eq!(req2.generation, initial_gen.wrapping_add(2));
        assert_eq!(req2.track_id, "trk-2");
    }

    #[test]
    fn begin_play_request_with_empty_queue_returns_none() {
        let mut state = test_wired();
        assert!(state.begin_play_request().is_none());
    }

    #[test]
    fn send_play_request_without_worker_plays_in_tests() {
        let mut state = test_wired();
        assert!(state.worker_tx.is_none());
        state.queue.push(Track::new("trk-wired", "Wired", "Artist"));
        assert!(state.request_current_play());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("trk-wired"));
    }

    #[test]
    fn sleep_timer_hook_blocks_play_request() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        assert!(state.request_current_play());
        assert!(state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));

        state
            .sleep_timer
            .set_preset_with_track(crate::domain::TimerPreset::StopAfterTrack, Some("t1"));
        assert!(state.sleep_timer.is_armed());

        state.queue.advance();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

        assert!(state.begin_play_request().is_none());
        let audio = state.audio.lock().unwrap();
        assert_eq!(audio.last_requested(), Some("t1"));
        assert!(!audio.is_playing());
    }

    #[test]
    fn sleep_timer_hook_does_not_block_regular_presets() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        assert!(state.request_current_play());
        let gen_after_play = state.play_generation;

        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());

        state.queue.advance();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

        let req = state.begin_play_request();
        assert!(req.is_some());
        assert_eq!(state.play_generation, gen_after_play.wrapping_add(1));
    }

    #[test]
    fn test_override_applies_to_track_and_prefetch_ids() {
        let mut state = test_wired();
        state.queue.clear();
        state.queue.push(Track::new("original", "Song", "Artist"));
        state.queue.push(Track::new("next-original", "Next", "Artist"));
        state.version_overrides.set("original", "fixed").expect("override");
        state.version_overrides.set("next-original", "next-fixed").expect("override");

        let request = state.begin_play_request().expect("request built");

        assert_eq!(request.track_id, "fixed");
        assert_eq!(request.prefetch_stream_id.as_deref(), Some("next-fixed"));
    }

    #[test]
    fn test_without_override_ids_are_unchanged() {
        let mut state = test_wired();
        state.queue.clear();
        state.queue.push(Track::new("plain", "Song", "Artist"));

        let request = state.begin_play_request().expect("request built");

        assert_eq!(request.track_id, "plain");
        assert_eq!(request.prefetch_stream_id, None);
    }

    #[test]
    fn play_request_after_ended_does_not_skip_the_rest_of_the_queue() {
        use crate::domain::PlaybackPhase;

        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(180));
        state.queue.push(Track::new("t3", "Track 3", "Artist 3").with_duration(180));
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("t1", 180))));

        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
        assert_eq!(state.playback_phase, PlaybackPhase::Resolving);

        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
    }
}
