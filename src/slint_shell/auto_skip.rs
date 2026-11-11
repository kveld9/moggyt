//! Community skip segment polling.
//!
//! Hexagonal invariant: remote SponsorBlock fetches execute on a background
//! worker thread and deliver results through an mpsc channel drained by the
//! UI timer, ensuring zero network I/O on the UI thread.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::domain::{SkipRanges, Track};
use crate::infra::SkipProvider;
use crate::slint_shell::state::SlintAppState;

/// Lifecycle of a community skip-range request for one video id.
///
/// Transitions are owned by [`SlintAppState::request_sponsor_ranges`] (start) and
/// [`SlintAppState::apply_sponsor_outcome`] (completion):
///
/// ```text
/// Absent -> Pending -> Available
///                  -> Failed -> Retrying -> Available
///                                        -> Abandoned
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SponsorRangeState {
    /// First fetch in flight.
    Pending,
    /// The single allowed retry is in flight.
    Retrying,
    /// Terminal: the provider answered. An empty set means the video has no segments.
    Available(SkipRanges),
    /// The first fetch failed; one retry is allowed.
    Failed,
    /// The retry failed; the video id is not requested again in this session.
    Abandoned,
}

/// Terminal result delivered by the background community skip worker.
#[derive(Debug, Clone)]
pub(crate) enum SponsorFetchOutcome {
    /// The provider answered, with the segments it found (possibly none).
    Ranges(SkipRanges),
    /// The provider failed; the lifecycle decides whether a retry is allowed.
    Failed,
}

impl SlintAppState {
    /// Drains completed community skip fetches and applies their lifecycle outcome.
    pub(crate) fn poll_sponsor_ranges(&mut self) {
        let Some(rx) = self.sponsor_rx.as_ref() else {
            return;
        };
        let mut received = Vec::new();
        while let Ok(item) = rx.try_recv() {
            received.push(item);
        }
        for (video_id, outcome) in received {
            self.apply_sponsor_outcome(video_id, outcome);
        }
    }

    /// Applies one terminal fetch outcome, keeping the per-video lifecycle total.
    fn apply_sponsor_outcome(&mut self, video_id: String, outcome: SponsorFetchOutcome) {
        if self.cached_skip_ranges.as_ref().is_some_and(|(id, _)| id == &video_id) {
            self.cached_skip_ranges = None;
        }
        let next = match (self.sponsor_range_states.get(&video_id), outcome) {
            (_, SponsorFetchOutcome::Ranges(ranges)) => SponsorRangeState::Available(ranges),
            (Some(SponsorRangeState::Pending), SponsorFetchOutcome::Failed) => {
                SponsorRangeState::Failed
            }
            _ => SponsorRangeState::Abandoned,
        };
        self.sponsor_range_states.insert(video_id, next);
    }

    /// True when the video has no answer yet, or holds one retry after a failure.
    pub(crate) fn should_request_sponsor_ranges(&self, video_id: &str) -> bool {
        matches!(self.sponsor_range_states.get(video_id), None | Some(SponsorRangeState::Failed))
    }

    /// Segments for a video id; available only once the provider answered.
    pub(crate) fn available_sponsor_ranges(&self, video_id: &str) -> Option<&SkipRanges> {
        match self.sponsor_range_states.get(video_id) {
            Some(SponsorRangeState::Available(ranges)) => Some(ranges),
            _ => None,
        }
    }

    /// Requests community skip ranges for the track, bounded to one retry per video id.
    pub(crate) fn request_sponsor_ranges(&mut self, track: &Track) {
        let Some(provider): Option<Arc<dyn SkipProvider>> =
            self.skip_provider.as_ref().map(Arc::clone)
        else {
            return;
        };
        if !self.should_request_sponsor_ranges(&track.id) {
            return;
        }
        let is_retry = self.sponsor_range_states.contains_key(&track.id);
        let sender = self.sponsor_sender();
        let video_id = track.id.clone();
        let duration_secs = track.duration_secs;
        let in_flight =
            if is_retry { SponsorRangeState::Retrying } else { SponsorRangeState::Pending };
        self.sponsor_range_states.insert(video_id.clone(), in_flight);
        if let Err(err) = spawn_sponsor_fetch(provider, sender, video_id.clone(), duration_secs) {
            tracing::warn!(error = %err, "failed to spawn community skip fetch thread");
            let failed =
                if is_retry { SponsorRangeState::Abandoned } else { SponsorRangeState::Failed };
            self.sponsor_range_states.insert(video_id, failed);
        }
    }

    pub(crate) fn sponsor_sender(&mut self) -> Sender<(String, SponsorFetchOutcome)> {
        if let Some(sender) = self.sponsor_tx.as_ref() {
            return sender.clone();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.sponsor_tx = Some(sender.clone());
        self.sponsor_rx = Some(receiver);
        sender
    }
}

/// Runs the provider lookup on a named worker thread and reports the terminal outcome.
fn spawn_sponsor_fetch(
    provider: Arc<dyn SkipProvider>,
    sender: Sender<(String, SponsorFetchOutcome)>,
    video_id: String,
    duration_secs: Option<u32>,
) -> std::io::Result<()> {
    std::thread::Builder::new().name("moggyt-sponsor-skip".to_owned()).spawn(move || {
        let outcome = match provider.fetch_ranges(&video_id, duration_secs) {
            Ok(ranges) => SponsorFetchOutcome::Ranges(ranges),
            Err(err) => {
                tracing::warn!(error = %err, "community skip fetch failed");
                SponsorFetchOutcome::Failed
            }
        };
        let _ = sender.send((video_id, outcome));
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::domain::{SkipRanges, SkipSegment, Track};
    use crate::infra::SkipStore;
    use crate::slint_shell::test_support::test_wired;

    use super::{SponsorFetchOutcome, SponsorRangeState};

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
        state.sponsor_range_states.insert(
            "t1".to_string(),
            SponsorRangeState::Available(SkipRanges(vec![SkipSegment::new(10, 20).unwrap()])),
        );
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
        state.sponsor_range_states.insert(
            "t1".to_string(),
            SponsorRangeState::Available(SkipRanges(vec![SkipSegment::new(10, 20).unwrap()])),
        );
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
        tx.send(("t1".to_string(), SponsorFetchOutcome::Ranges(ranges.clone()))).unwrap();
        state.poll_sponsor_ranges();
        assert_eq!(state.available_sponsor_ranges("t1"), Some(&ranges));
        assert!(state.cached_skip_ranges.is_none());
    }

    #[test]
    fn test_should_request_sponsor_ranges_guard() {
        let mut state = test_wired();
        assert!(state.should_request_sponsor_ranges("fresh"));

        state.sponsor_range_states.insert("pending".to_string(), SponsorRangeState::Pending);
        assert!(!state.should_request_sponsor_ranges("pending"));

        state.sponsor_range_states.insert("retrying".to_string(), SponsorRangeState::Retrying);
        assert!(!state.should_request_sponsor_ranges("retrying"));

        state
            .sponsor_range_states
            .insert("cached".to_string(), SponsorRangeState::Available(SkipRanges::new()));
        assert!(!state.should_request_sponsor_ranges("cached"));

        state.sponsor_range_states.insert("abandoned".to_string(), SponsorRangeState::Abandoned);
        assert!(!state.should_request_sponsor_ranges("abandoned"));

        state.sponsor_range_states.insert("failed".to_string(), SponsorRangeState::Failed);
        assert!(state.should_request_sponsor_ranges("failed"));
    }

    #[test]
    fn test_sponsor_failure_allows_exactly_one_retry() {
        let mut state = test_wired();
        state.sponsor_range_states.insert("t1".to_string(), SponsorRangeState::Pending);
        state.apply_sponsor_outcome("t1".to_string(), SponsorFetchOutcome::Failed);
        assert!(state.should_request_sponsor_ranges("t1"));
        assert_eq!(state.sponsor_range_states.get("t1"), Some(&SponsorRangeState::Failed));

        state.sponsor_range_states.insert("t1".to_string(), SponsorRangeState::Retrying);
        state.apply_sponsor_outcome("t1".to_string(), SponsorFetchOutcome::Failed);
        assert!(!state.should_request_sponsor_ranges("t1"));
        assert_eq!(state.sponsor_range_states.get("t1"), Some(&SponsorRangeState::Abandoned));
    }

    #[test]
    fn test_empty_sponsor_ranges_are_terminal() {
        let mut state = test_wired();
        state.sponsor_range_states.insert("t1".to_string(), SponsorRangeState::Pending);
        state.apply_sponsor_outcome(
            "t1".to_string(),
            SponsorFetchOutcome::Ranges(SkipRanges::new()),
        );
        assert!(!state.should_request_sponsor_ranges("t1"));
        assert_eq!(state.available_sponsor_ranges("t1"), Some(&SkipRanges::new()));
    }
}
