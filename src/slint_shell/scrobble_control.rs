//! Scrobble submission and presence sync for the playing track.

use crate::infra::{Presence, should_scrobble};
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Submits scrobble record to active scrobbler if playback elapsed threshold was met.
    pub fn maybe_scrobble_current(&mut self) {
        let Some(started) = self.started_at.take() else {
            return;
        };
        let elapsed = started.listened().as_secs();
        self.record_play(&started, elapsed);
        let duration = started.track.duration_secs.map(u64::from);
        if should_scrobble(elapsed, duration) {
            self.submit_scrobble(&started.track.artist, &started.track.title, started.unix_secs);
        }
    }

    /// Freezes listened-time accumulation for the active track.
    pub fn pause_play_ledger(&mut self) {
        if let Some(started) = self.started_at.as_mut() {
            started.on_pause();
        }
    }

    /// Restarts listened-time accumulation for the active track.
    pub fn resume_play_ledger(&mut self) {
        if let Some(started) = self.started_at.as_mut() {
            started.on_resume();
        }
    }

    fn submit_scrobble(&self, artist: &str, title: &str, unix_secs: u64) {
        if let Ok(mut s) = self.scrobbler.lock() {
            s.scrobble(artist, title, unix_secs);
        }
        if let Some(sec) = &self.secondary_scrobbler {
            if let Ok(mut s) = sec.lock() {
                s.scrobble(artist, title, unix_secs);
            }
        }
    }

    /// Synchronizes active track metadata with desktop presence and Discord presence.
    pub fn sync_presence_track(&mut self, artist: &str, title: &str, is_playing: bool) {
        let key = (artist.to_string(), title.to_string());
        if self.last_presence_key.as_ref() != Some(&key) {
            self.presence.set_track(artist, title);
            self.discord.set_track(artist, title);
            self.last_presence_key = Some(key);
        }
        self.presence.set_playing(is_playing);
    }

    /// Synchronizes playback status with desktop presence.
    pub fn sync_presence_playing(&mut self, is_playing: bool) {
        self.presence.set_playing(is_playing);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::domain::Track;
    use crate::infra::{MemoryPlayLogStore, PlayLogStore, Scrobbler};
    use crate::slint_shell::playback_control::StartedPlay;
    use crate::slint_shell::test_support::{EndedAudio, test_wired};

    #[derive(Debug)]
    struct RecordingScrobbler {
        scrobbles: Arc<Mutex<Vec<(String, String, u64)>>>,
    }

    impl Scrobbler for RecordingScrobbler {
        fn now_playing(&mut self, _artist: &str, _title: &str) {}

        fn scrobble(&mut self, artist: &str, title: &str, timestamp_secs: u64) {
            self.scrobbles.lock().unwrap().push((
                artist.to_string(),
                title.to_string(),
                timestamp_secs,
            ));
        }
    }

    #[test]
    fn test_maybe_scrobble_current_credits_started_track_after_queue_advance() {
        let mut state = test_wired();
        let track_a = Track::new("a", "A title", "A artist").with_duration(200);
        let track_b = Track::new("b", "B title", "B artist").with_duration(200);
        state.queue.push(track_a.clone());
        state.queue.push(track_b);

        let scrobbles = Arc::new(Mutex::new(Vec::new()));
        let rec = RecordingScrobbler { scrobbles: Arc::clone(&scrobbles) };
        state.scrobbler = Arc::new(Mutex::new(Box::new(rec)));
        state.started_at = Some(StartedPlay {
            unix_secs: 1_000,
            track: track_a,
            listened: Duration::from_secs(150),
            resumed_at: None,
        });

        state.queue.advance();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("b"));

        state.maybe_scrobble_current();

        assert_eq!(
            *scrobbles.lock().unwrap(),
            vec![("A artist".to_string(), "A title".to_string(), 1_000)]
        );
        assert!(state.started_at.is_none());
    }

    #[test]
    fn test_maybe_scrobble_current_below_threshold_does_not_scrobble_and_resets() {
        let mut state = test_wired();
        let track_a = Track::new("a", "A title", "A artist").with_duration(200);
        let track_b = Track::new("b", "B title", "B artist").with_duration(200);
        state.queue.push(track_a.clone());
        state.queue.push(track_b);

        let scrobbles = Arc::new(Mutex::new(Vec::new()));
        let rec = RecordingScrobbler { scrobbles: Arc::clone(&scrobbles) };
        state.scrobbler = Arc::new(Mutex::new(Box::new(rec)));
        state.started_at = Some(StartedPlay {
            unix_secs: 1_000,
            track: track_a,
            listened: Duration::from_secs(10),
            resumed_at: None,
        });

        state.queue.advance();
        state.maybe_scrobble_current();

        assert!(scrobbles.lock().unwrap().is_empty());
        assert!(state.started_at.is_none());
    }

    #[test]
    fn pause_time_not_counted() {
        let track = Track::new("t1", "Title", "Artist").with_duration(200);
        let mut started = StartedPlay::now(track);
        started.resumed_at =
            Some(Instant::now().checked_sub(Duration::from_secs(20)).expect("valid instant"));
        started.on_pause();
        assert_eq!(started.listened().as_secs(), 20);

        assert!(started.resumed_at.is_none());

        started.resumed_at =
            Some(Instant::now().checked_sub(Duration::from_secs(5)).expect("valid instant"));
        assert_eq!(started.listened().as_secs(), 25);
    }

    #[test]
    fn last_track_credited_on_end() {
        let mut state = test_wired();
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("t1", 200))));
        let track = Track::new("t1", "Title 1", "Artist 1").with_duration(200);
        state.queue.push(track.clone());
        state.started_at = Some(StartedPlay {
            unix_secs: 1,
            track,
            listened: Duration::from_secs(120),
            resumed_at: None,
        });
        state.autoplay = false;
        let memory_store = MemoryPlayLogStore::new();
        state.play_log_store = Box::new(memory_store.clone());

        state.continue_autoplay_if_ended();

        let records = memory_store.load().expect("load succeeds");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].listened_secs, 120);
        assert!(state.started_at.is_none());
    }

    #[test]
    fn below_threshold_not_scrobbled() {
        let mut state = test_wired();
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("t1", 200))));
        let track = Track::new("t1", "Title 1", "Artist 1").with_duration(200);
        state.queue.push(track.clone());
        state.started_at = Some(StartedPlay {
            unix_secs: 1,
            track,
            listened: Duration::from_secs(10),
            resumed_at: None,
        });
        state.autoplay = false;
        let memory_store = MemoryPlayLogStore::new();
        state.play_log_store = Box::new(memory_store.clone());

        state.continue_autoplay_if_ended();

        let records = memory_store.load().expect("load succeeds");
        assert!(records.is_empty());
        assert!(state.started_at.is_none());
    }
}
