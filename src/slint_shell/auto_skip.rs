//! Community skip segment polling.
//!
//! Hexagonal invariant: remote SponsorBlock fetches execute on a background
//! worker thread and deliver results through an mpsc channel drained by the
//! UI timer, ensuring zero network I/O on the UI thread.

use std::sync::Arc;

use crate::domain::{SkipRanges, Track};
use crate::infra::SkipProvider;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Drains completed community skip fetches, invalidating the cached range of the
    /// affected track so the next auto-skip evaluation sees the new segments.
    pub(crate) fn poll_sponsor_ranges(&mut self) {
        let Some(rx) = self.sponsor_rx.as_ref() else {
            return;
        };
        let mut received = Vec::new();
        while let Ok(item) = rx.try_recv() {
            received.push(item);
        }
        for (video_id, ranges) in received {
            if self.cached_skip_ranges.as_ref().is_some_and(|(id, _)| id == &video_id) {
                self.cached_skip_ranges = None;
            }
            self.sponsor_ranges.insert(video_id, ranges);
        }
    }

    pub(crate) fn should_request_sponsor_ranges(&self, video_id: &str) -> bool {
        !self.sponsor_requested.contains(video_id) && !self.sponsor_ranges.contains_key(video_id)
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub(crate) fn request_sponsor_ranges(&mut self, track: &Track) {
        let Some(provider): Option<Arc<dyn SkipProvider>> =
            self.skip_provider.as_ref().map(Arc::clone)
        else {
            return;
        };
        if !self.should_request_sponsor_ranges(&track.id) {
            return;
        }
        let sender = self.sponsor_sender();
        let video_id = track.id.clone();
        let duration_secs = track.duration_secs;
        self.sponsor_requested.insert(video_id.clone());
        let spawned =
            std::thread::Builder::new().name("moggyt-sponsor-skip".to_owned()).spawn(move || {
                match provider.fetch_ranges(&video_id, duration_secs) {
                    Ok(ranges) if !ranges.is_empty() => {
                        let _ = sender.send((video_id, ranges));
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(error = %err, "community skip fetch failed"),
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(error = %err, "failed to spawn community skip fetch thread");
        }
    }

    pub(crate) fn sponsor_sender(&mut self) -> std::sync::mpsc::Sender<(String, SkipRanges)> {
        if let Some(sender) = self.sponsor_tx.as_ref() {
            return sender.clone();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.sponsor_tx = Some(sender.clone());
        self.sponsor_rx = Some(receiver);
        sender
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::{SkipRanges, SkipSegment, Track};
    use crate::infra::SkipStore;
    use crate::slint_shell::test_support::test_wired;

    #[derive(Debug)]
    struct CountingSkipStore {
        load_count: Arc<AtomicUsize>,
        ranges: HashMap<String, SkipRanges>,
    }

    impl SkipStore for CountingSkipStore {
        fn load(&self) -> HashMap<String, SkipRanges> {
            self.load_count.fetch_add(1, Ordering::SeqCst);
            self.ranges.clone()
        }

        fn save(&self, _map: &HashMap<String, SkipRanges>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_auto_skip_evaluation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        let mut ranges = crate::domain::SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());
        let mut map = std::collections::HashMap::new();
        map.insert("t1".to_string(), ranges);
        let _ = state.skip_store.save(&map);

        state.seek_by(12);
        state.check_auto_skip();
        assert_eq!(state.audio.lock().unwrap().position_secs(), 20);
    }

    #[test]
    fn test_auto_skip_repeated_checks_load_once() {
        let mut state = test_wired();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());
        let mut map = HashMap::new();
        map.insert("t1".to_string(), ranges);

        state.skip_store =
            Box::new(CountingSkipStore { load_count: Arc::clone(&counter), ranges: map });

        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.check_auto_skip();
        state.check_auto_skip();
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_auto_skip_switching_track_loads_again() {
        let mut state = test_wired();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut ranges1 = SkipRanges::new();
        ranges1.insert(SkipSegment::new(10, 20).unwrap());
        let mut ranges2 = SkipRanges::new();
        ranges2.insert(SkipSegment::new(30, 40).unwrap());
        let mut map = HashMap::new();
        map.insert("t1".to_string(), ranges1);
        map.insert("t2".to_string(), ranges2);

        state.skip_store =
            Box::new(CountingSkipStore { load_count: Arc::clone(&counter), ranges: map });

        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));
        state.queue.advance();
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_auto_skip_uses_sponsor_ranges_when_store_is_empty() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state
            .sponsor_ranges
            .insert("t1".to_string(), SkipRanges(vec![SkipSegment::new(10, 20).unwrap()]));
        state.seek_by(12);
        state.check_auto_skip();
        assert_eq!(state.audio.lock().unwrap().position_secs(), 20);
    }

    #[test]
    fn test_auto_skip_prefers_local_ranges_over_sponsor_ranges() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        let map = HashMap::from([(
            "t1".to_string(),
            SkipRanges(vec![SkipSegment::new(10, 15).unwrap()]),
        )]);
        let _ = state.skip_store.save(&map);
        state
            .sponsor_ranges
            .insert("t1".to_string(), SkipRanges(vec![SkipSegment::new(10, 20).unwrap()]));
        state.seek_by(12);
        state.check_auto_skip();
        assert_eq!(state.audio.lock().unwrap().position_secs(), 15);
    }

    #[test]
    fn test_poll_sponsor_ranges_drains_channel_and_invalidates_cache() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel();
        state.sponsor_tx = Some(tx.clone());
        state.sponsor_rx = Some(rx);
        state.cached_skip_ranges = Some(("t1".to_string(), None));
        let ranges = SkipRanges(vec![SkipSegment::new(10, 20).unwrap()]);
        tx.send(("t1".to_string(), ranges.clone())).unwrap();
        state.poll_sponsor_ranges();
        assert_eq!(state.sponsor_ranges.get("t1"), Some(&ranges));
        assert!(state.cached_skip_ranges.is_none());
    }

    #[test]
    fn test_should_request_sponsor_ranges_guard() {
        let mut state = test_wired();
        assert!(state.should_request_sponsor_ranges("fresh"));
        state.sponsor_requested.insert("req".to_string());
        assert!(!state.should_request_sponsor_ranges("req"));
        state.sponsor_ranges.insert("cached".to_string(), SkipRanges::new());
        assert!(!state.should_request_sponsor_ranges("cached"));
    }
}
