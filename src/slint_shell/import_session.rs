//! In-memory state machine for Spotify playlist import runs.
//!
//! Hexagonal invariant: pure state machine with zero Slint dependencies, zero I/O,
//! zero network, and zero thread spawning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::domain::{LocalPlaylist, SpotifyPlaylist};
use crate::infra::PlaylistStore;
use crate::slint_shell::import_state::{ImportCounts, ImportReview};
use crate::slint_shell::import_worker::{ImportProgressEvent, TrackResolution};

/// Stage of the import flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportStage {
    #[default]
    Idle,
    LoadingSource,
    Matching,
    Review,
    Failed,
}

/// What a `drain` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrainOutcome {
    pub applied: usize,
    pub finished: bool,
    pub cancelled: bool,
}

/// Formats the progress status text for matching progress.
pub fn progress_status(done: usize, total: usize) -> String {
    format!("{done} / {total}")
}

/// One import run owned by the UI.
pub struct ImportSession {
    pub stage: ImportStage,
    pub source: Option<SpotifyPlaylist>,
    pub review: Option<ImportReview>,
    pub status: String,
    pub picker_row: Option<usize>,
    cancel: Arc<AtomicBool>,
    receiver: Option<mpsc::Receiver<ImportProgressEvent>>,
    played_events: usize,
    resolutions: Vec<TrackResolution>,
}

impl Default for ImportSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportSession {
    /// Creates a fresh idle import session.
    pub fn new() -> Self {
        Self {
            stage: ImportStage::Idle,
            source: None,
            review: None,
            status: String::new(),
            picker_row: None,
            cancel: Arc::new(AtomicBool::new(false)),
            receiver: None,
            played_events: 0,
            resolutions: Vec::new(),
        }
    }

    /// Transitions to `LoadingSource` and clears any previous review or cancel flag.
    pub fn start_loading(&mut self) {
        self.stage = ImportStage::LoadingSource;
        self.source = None;
        self.review = None;
        self.status.clear();
        self.picker_row = None;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.receiver = None;
        self.played_events = 0;
        self.resolutions.clear();
    }

    /// Sets the session to `Failed` stage with the given message.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.stage = ImportStage::Failed;
        self.status = message.into();
    }

    /// Begins matching: stores the source, swaps in a fresh cancel flag and event receiver, stage Matching.
    pub fn begin_matching(
        &mut self,
        source: SpotifyPlaylist,
        receiver: mpsc::Receiver<ImportProgressEvent>,
    ) -> Arc<AtomicBool> {
        self.stage = ImportStage::Matching;
        self.source = Some(source);
        self.review = None;
        self.resolutions.clear();
        self.picker_row = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Arc::clone(&cancel);
        self.receiver = Some(receiver);
        cancel
    }

    /// Replaces the cancel flag (useful when wiring background workers).
    pub fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
    }

    /// Returns a clone of the cancel atomic handle.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// Returns the number of events drained and applied so far.
    pub fn played_events(&self) -> usize {
        self.played_events
    }

    /// Requests cancellation for the running worker.
    pub fn request_cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Returns `true` if matching is actively running.
    pub fn is_running(&self) -> bool {
        matches!(self.stage, ImportStage::Matching)
    }

    /// Returns tally counts from the review, or default zeros if no review exists.
    pub fn counts(&self) -> ImportCounts {
        self.review.as_ref().map(|r| r.counts()).unwrap_or_default()
    }

    /// Opens the alternative picker for the specified row.
    pub fn open_picker(&mut self, row: usize) -> bool {
        let Some(ref rev) = self.review else {
            return false;
        };
        if row >= rev.rows.len() {
            return false;
        }
        self.picker_row = Some(row);
        true
    }

    /// Closes the alternative picker.
    pub fn close_picker(&mut self) {
        self.picker_row = None;
    }

    /// Chooses an alternative for the picker's row and closes the picker.
    pub fn choose_alternative(&mut self, alternative: usize) -> bool {
        let Some(row) = self.picker_row else {
            return false;
        };
        let ok = self.review.as_mut().map(|r| r.swap(row, alternative)).unwrap_or(false);
        self.close_picker();
        ok
    }

    /// Selects an active review row.
    pub fn select_row(&mut self, row: usize) -> bool {
        self.review.as_mut().map(|r| r.select(row)).unwrap_or(false)
    }

    /// Saves the reviewed playlist through the store and returns it.
    pub fn save(&mut self, store: &dyn PlaylistStore) -> anyhow::Result<LocalPlaylist> {
        let Some(ref rev) = self.review else {
            anyhow::bail!("no import review available to save");
        };
        if !rev.has_matches() {
            anyhow::bail!("no matched tracks to save");
        }
        let playlist = rev.to_local_playlist();
        store.save(&playlist)?;
        Ok(playlist)
    }

    /// Non-blocking drain of every queued event.
    pub fn drain(&mut self) -> DrainOutcome {
        let mut outcome = DrainOutcome::default();
        let Some(rx) = self.receiver.take() else {
            return outcome;
        };
        let still_connected = self.drain_loop(&rx, &mut outcome);
        if !outcome.finished && still_connected {
            self.receiver = Some(rx);
        }
        outcome
    }

    fn drain_loop(
        &mut self,
        rx: &mpsc::Receiver<ImportProgressEvent>,
        outcome: &mut DrainOutcome,
    ) -> bool {
        loop {
            match rx.try_recv() {
                Ok(event) => self.apply_event(event, outcome),
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => return false,
            }
            if outcome.finished {
                return false;
            }
        }
    }

    fn apply_event(&mut self, event: ImportProgressEvent, outcome: &mut DrainOutcome) {
        outcome.applied += 1;
        self.played_events += 1;
        match event {
            ImportProgressEvent::Started { .. } => {}
            ImportProgressEvent::Track { index, resolution } => {
                self.record_resolution(index, resolution);
            }
            ImportProgressEvent::Progress { done, total, .. } => {
                self.status = progress_status(done, total);
            }
            ImportProgressEvent::Finished { .. } => {
                self.finish_review(outcome, false);
            }
            ImportProgressEvent::Cancelled { .. } => {
                self.finish_review(outcome, true);
            }
            ImportProgressEvent::Failed { message } => {
                self.fail(message);
                outcome.finished = true;
            }
        }
    }

    fn record_resolution(&mut self, index: usize, resolution: TrackResolution) {
        if index >= self.resolutions.len() {
            self.resolutions.resize(
                index + 1,
                TrackResolution::Unresolved {
                    title: String::new(),
                    artist: String::new(),
                    reason: "unresolved".into(),
                },
            );
        }
        self.resolutions[index] = resolution;
        if let Some(ref src) = self.source {
            self.review = Some(ImportReview::from_resolutions(src, &self.resolutions));
        }
    }

    fn finish_review(&mut self, outcome: &mut DrainOutcome, cancelled: bool) {
        self.stage = ImportStage::Review;
        if let Some(ref src) = self.source {
            self.review = Some(ImportReview::from_resolutions(src, &self.resolutions));
        }
        outcome.finished = true;
        if cancelled {
            outcome.cancelled = true;
            self.status = "Import cancelled".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchConfidence, SpotifyTrack};
    use crate::infra::MemoryPlaylistStore;

    fn sample_spotify_playlist() -> SpotifyPlaylist {
        SpotifyPlaylist {
            id: "sp_test_id".to_string(),
            title: "Road Trip".to_string(),
            author: "Tester".to_string(),
            artwork: Some("https://example.com/cover.jpg".to_string()),
            tracks: vec![
                SpotifyTrack {
                    title: "Song 1".to_string(),
                    artist: "Artist 1".to_string(),
                    album: None,
                    duration_ms: 180000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song 2".to_string(),
                    artist: "Artist 2".to_string(),
                    album: None,
                    duration_ms: 200000,
                    isrc: None,
                },
            ],
        }
    }

    #[test]
    fn test_progress_status_formats_done_over_total() {
        assert_eq!(progress_status(3, 10), "3 / 10");
        assert_eq!(progress_status(0, 0), "0 / 0");
    }

    #[test]
    fn test_new_session_is_idle_with_defaults() {
        let session = ImportSession::new();
        assert_eq!(session.stage, ImportStage::Idle);
        assert!(session.source.is_none());
        assert!(session.review.is_none());
        assert!(session.status.is_empty());
        assert_eq!(session.picker_row, None);
        assert!(!session.is_running());
        assert_eq!(session.played_events(), 0);
        assert_eq!(session.counts(), ImportCounts::default());
    }

    #[test]
    fn test_start_loading_transitions_stage_and_clears_state() {
        let mut session = ImportSession::new();
        session.fail("prior error");
        session.request_cancel();
        assert!(session.cancel_flag().load(Ordering::Relaxed));

        session.start_loading();
        assert_eq!(session.stage, ImportStage::LoadingSource);
        assert!(session.status.is_empty());
        assert!(!session.cancel_flag().load(Ordering::Relaxed));
    }

    #[test]
    fn test_fail_sets_failed_stage_and_status() {
        let mut session = ImportSession::new();
        session.fail("connection timeout");
        assert_eq!(session.stage, ImportStage::Failed);
        assert_eq!(session.status, "connection timeout");
    }

    #[test]
    fn test_begin_matching_and_request_cancel() {
        let mut session = ImportSession::new();
        let (_tx, rx) = mpsc::channel();
        let cancel = session.begin_matching(sample_spotify_playlist(), rx);
        assert_eq!(session.stage, ImportStage::Matching);
        assert!(session.is_running());
        assert!(!cancel.load(Ordering::Relaxed));

        session.request_cancel();
        assert!(cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn test_drain_empty_and_disconnected() {
        let mut session = ImportSession::new();
        let outcome = session.drain();
        assert_eq!(outcome.applied, 0);
        assert!(!outcome.finished);

        let (tx, rx) = mpsc::channel::<ImportProgressEvent>();
        session.begin_matching(sample_spotify_playlist(), rx);
        drop(tx);
        let outcome2 = session.drain();
        assert_eq!(outcome2.applied, 0);
        assert!(!outcome2.finished);
    }

    #[test]
    fn test_drain_applies_track_progress_and_finished_events() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        let local = LocalTrack::new("v1", "Song 1", "Artist 1", MatchConfidence::High);
        tx.send(ImportProgressEvent::Started { total: 2 }).unwrap();
        tx.send(ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Matched {
                track: local.clone(),
                alternatives: vec![local.clone()],
            },
        })
        .unwrap();
        tx.send(ImportProgressEvent::Progress {
            done: 1,
            total: 2,
            high: 1,
            review: 0,
            unresolved: 0,
        })
        .unwrap();
        tx.send(ImportProgressEvent::Finished {
            playlist: LocalPlaylist::new("local:sp_test_id", "Road Trip"),
        })
        .unwrap();

        let outcome = session.drain();
        assert_eq!(outcome.applied, 4);
        assert!(outcome.finished);
        assert!(!outcome.cancelled);
        assert_eq!(session.stage, ImportStage::Review);
        assert_eq!(session.status, "1 / 2");

        let rev = session.review.as_ref().expect("review created");
        assert_eq!(rev.rows.len(), 1);
        assert_eq!(rev.rows[0].source_title, "Song 1");
        assert_eq!(session.counts().high, 1);
    }

    #[test]
    fn test_drain_cancelled_keeps_partial_rows_and_sets_status() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        let local = LocalTrack::new("v1", "Song 1", "Artist 1", MatchConfidence::High);
        tx.send(ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Matched {
                track: local.clone(),
                alternatives: vec![local.clone()],
            },
        })
        .unwrap();
        tx.send(ImportProgressEvent::Cancelled {
            playlist: LocalPlaylist::new("local:sp_test_id", "Road Trip"),
        })
        .unwrap();

        let outcome = session.drain();
        assert_eq!(outcome.applied, 2);
        assert!(outcome.finished);
        assert!(outcome.cancelled);
        assert_eq!(session.stage, ImportStage::Review);
        assert_eq!(session.status, "Import cancelled");

        let rev = session.review.as_ref().expect("review created");
        assert_eq!(rev.rows.len(), 1);
    }

    #[test]
    fn test_drain_failed_event_sets_failed_stage() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        tx.send(ImportProgressEvent::Failed { message: "rate limit exhausted".into() }).unwrap();

        let outcome = session.drain();
        assert_eq!(outcome.applied, 1);
        assert!(outcome.finished);
        assert_eq!(session.stage, ImportStage::Failed);
        assert_eq!(session.status, "rate limit exhausted");
    }

    #[test]
    fn test_picker_open_choose_close_lifecycle() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        let t1 = LocalTrack::new("v1", "Song 1", "Artist 1", MatchConfidence::High);
        let alt1 = LocalTrack::new("v2", "Song 1 Alt", "Artist 1", MatchConfidence::Medium);
        tx.send(ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Matched {
                track: t1.clone(),
                alternatives: vec![t1, alt1],
            },
        })
        .unwrap();
        tx.send(ImportProgressEvent::Finished {
            playlist: LocalPlaylist::new("local:sp_test_id", "Road Trip"),
        })
        .unwrap();
        session.drain();

        assert!(!session.open_picker(10));
        assert!(session.open_picker(0));
        assert_eq!(session.picker_row, Some(0));

        assert!(session.select_row(0));
        assert!(!session.select_row(99));

        assert!(session.choose_alternative(1));
        assert_eq!(session.picker_row, None);
        assert_eq!(
            session.review.as_ref().unwrap().rows[0].chosen.as_ref().map(|t| t.video_id.as_str()),
            Some("v2")
        );

        session.open_picker(0);
        session.close_picker();
        assert_eq!(session.picker_row, None);
    }

    #[test]
    fn test_save_writes_through_memory_store_and_errors_without_matches() {
        let store = MemoryPlaylistStore::new();
        let mut session = ImportSession::new();

        // No review
        assert!(session.save(&store).is_err());

        // Review with no matches
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);
        tx.send(ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Unresolved {
                title: "Song 1".into(),
                artist: "Artist 1".into(),
                reason: "not found".into(),
            },
        })
        .unwrap();
        tx.send(ImportProgressEvent::Finished {
            playlist: LocalPlaylist::new("local:sp_test_id", "Road Trip"),
        })
        .unwrap();
        session.drain();

        let err = session.save(&store).unwrap_err();
        assert!(err.to_string().contains("no matched tracks"));

        // Add a match
        let local = LocalTrack::new("v1", "Song 1", "Artist 1", MatchConfidence::High);
        session.review.as_mut().unwrap().rows[0].chosen = Some(local);
        session.review.as_mut().unwrap().rows[0].confidence = Some(MatchConfidence::High);

        let saved = session.save(&store).expect("save succeeds");
        assert_eq!(saved.id, "local:sp_test_id");
        assert_eq!(saved.title, "Road Trip");
        assert_eq!(store.get("local:sp_test_id").unwrap().tracks.len(), 1);
    }
}
