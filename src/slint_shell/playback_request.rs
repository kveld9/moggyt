//! Single playback transition request builder and dispatch authority.

#[cfg(test)]
use crate::slint_shell::playback_worker::PlaybackWorkerAdapters;
use crate::slint_shell::playback_worker::{
    LyricsPrefetch, PlayTrackRequest, PlaybackWorkerCommand,
};
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Constructs a play request for the current track and bumps the playback generation.
    pub fn begin_play_request(&mut self) -> Option<PlayTrackRequest> {
        let track = self.queue.current().cloned()?;
        self.play_generation = self.play_generation.wrapping_add(1);
        let generation = self.play_generation;
        self.maybe_scrobble_current();

        let next_track = self.queue.peek_next().filter(|next| next.id != track.id);
        let prefetch_lyrics = next_track.as_ref().map(|next| LyricsPrefetch {
            artist: next.artist.clone(),
            title: next.title.clone(),
            duration_secs: next.duration_secs,
        });
        let prefetch_stream_id = next_track.map(|next| next.id.clone());

        Some(PlayTrackRequest {
            generation,
            track_id: track.id,
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
    use crate::domain::Track;
    use crate::slint_shell::test_support::test_wired;

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
}
