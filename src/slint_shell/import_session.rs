//! In-memory state machine for Spotify playlist import runs.
//!
//! Hexagonal invariant: pure state machine with zero Slint dependencies, zero I/O,
//! zero network, and zero thread spawning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::domain::{LocalPlaylist, LocalTrack, SpotifyPlaylist};
use crate::infra::PlaylistStore;
use crate::slint_shell::import_state::{ImportCounts, ImportReview};
use crate::slint_shell::import_worker::{ImportProgressEvent, TrackResolution};

/// Destination for imported tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportDestination {
    #[default]
    Library,
    Queue,
}

/// Stage of the import flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportStage {
    #[default]
    Idle,
    LoadingSource,
    Matching,
    Cancelling,
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
#[derive(Default)]
pub struct ImportSession {
    pub stage: ImportStage,
    pub source: Option<SpotifyPlaylist>,
    pub review: Option<ImportReview>,
    pub status: String,
    pub picker_row: Option<usize>,
    pub destination: ImportDestination,
    pub name_override: Option<String>,
    load_generation: u64,
    cancel: Arc<AtomicBool>,
    receiver: Option<mpsc::Receiver<ImportProgressEvent>>,
    played_events: usize,
    resolutions: Vec<TrackResolution>,
    progress_done: usize,
    progress_total: usize,
}

impl ImportSession {
    /// Creates a fresh idle import session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Transitions to `LoadingSource`, increments generation, and clears any previous review or cancel flag.
    pub fn start_loading(&mut self) -> u64 {
        let (dest, name, generation) =
            (self.destination, self.name_override.take(), self.load_generation.wrapping_add(1));
        *self = Self::default();
        self.stage = ImportStage::LoadingSource;
        self.destination = dest;
        self.name_override = name;
        self.load_generation = generation;
        generation
    }

    /// Returns `true` if loading source, matching, or cancelling is actively running.
    pub fn is_busy(&self) -> bool {
        matches!(
            self.stage,
            ImportStage::LoadingSource | ImportStage::Matching | ImportStage::Cancelling
        )
    }

    /// Returns `true` only when stage is `LoadingSource` and generation matches.
    pub fn accepts_load_result(&self, generation: u64) -> bool {
        matches!(self.stage, ImportStage::LoadingSource) && self.load_generation == generation
    }

    /// Resets the session to `Idle`, requests cancellation, and invalidates in-flight loaders.
    pub fn reset_to_idle(&mut self) {
        let generation = self.load_generation.wrapping_add(1);
        *self = Self::default();
        self.request_cancel();
        self.load_generation = generation;
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
        self.progress_total = source.tracks.len();
        self.progress_done = 0;
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
        if self.stage == ImportStage::Matching {
            self.stage = ImportStage::Cancelling;
            self.status = "Import cancelled".to_string();
        }
    }

    /// Returns `true` if matching is actively running.
    pub fn is_running(&self) -> bool {
        matches!(self.stage, ImportStage::Matching)
    }

    /// Returns tally counts from the review, or default zeros if no review exists.
    pub fn counts(&self) -> ImportCounts {
        self.review.as_ref().map(|r| r.counts()).unwrap_or_default()
    }

    /// Returns the progress of the import run as `(done, total)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.progress_done, self.progress_total)
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

    /// Sets the import destination and optional playlist name override.
    pub fn set_options(&mut self, destination: ImportDestination, name_override: Option<String>) {
        self.destination = destination;
        self.name_override = name_override;
    }

    /// Returns the configured destination.
    pub fn destination(&self) -> ImportDestination {
        self.destination
    }

    /// Returns the chosen track of every review row in source order.
    pub fn resolved_tracks(&self) -> Vec<LocalTrack> {
        self.review.as_ref().map(|r| r.selected_tracks()).unwrap_or_default()
    }

    /// Returns the review artwork if a review exists.
    pub fn artwork(&self) -> Option<String> {
        self.review.as_ref().and_then(|r| r.artwork.clone())
    }

    /// Saves the reviewed playlist through the store and returns it.
    pub fn save(&mut self, store: &dyn PlaylistStore) -> anyhow::Result<LocalPlaylist> {
        let Some(ref rev) = self.review else {
            anyhow::bail!("no import review available to save");
        };
        if !rev.has_matches() {
            anyhow::bail!("no matched tracks to save");
        }
        let Some(ref source) = self.source else {
            anyhow::bail!("no import source available");
        };
        if rev.rows.len() != source.tracks.len() {
            anyhow::bail!("import review is incomplete");
        }
        let playlist = rev.to_local_playlist_with_title(self.name_override.as_deref());
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

    fn on_started(&mut self, total: usize) {
        if self.source.is_none() {
            self.progress_total = total;
        }
    }

    fn on_progress(&mut self, done: usize) {
        self.progress_done = done.max(self.progress_done);
        if self.stage != ImportStage::Cancelling {
            self.status = progress_status(self.progress_done, self.progress_total);
        }
    }

    fn apply_event(&mut self, event: ImportProgressEvent, outcome: &mut DrainOutcome) {
        outcome.applied += 1;
        self.played_events += 1;
        match event {
            ImportProgressEvent::Started { total } => self.on_started(total),
            ImportProgressEvent::Track { index, resolution } => {
                self.record_resolution(index, resolution)
            }
            ImportProgressEvent::Progress { done, .. } => self.on_progress(done),
            ImportProgressEvent::Finished { .. } => self.finish_review(outcome),
            ImportProgressEvent::Cancelled { .. } => {
                self.reset_to_idle();
                outcome.finished = true;
                outcome.cancelled = true;
            }
            ImportProgressEvent::Failed { message } => {
                self.fail(message);
                outcome.finished = true;
            }
        }
    }

    fn sync_review(&mut self) {
        if let Some(ref src) = self.source {
            self.review = Some(ImportReview::from_resolutions(src, &self.resolutions));
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
        self.sync_review();
    }

    fn finish_review(&mut self, outcome: &mut DrainOutcome) {
        self.stage = ImportStage::Review;
        self.sync_review();
        outcome.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchConfidence, SpotifyTrack};
    use crate::infra::MemoryPlaylistStore;

    fn sample_playlist_with_tracks(count: usize) -> SpotifyPlaylist {
        let tracks = (0..count)
            .map(|i| SpotifyTrack {
                title: format!("Song {}", i + 1),
                artist: format!("Artist {}", i + 1),
                album: None,
                duration_ms: 180000,
                isrc: None,
            })
            .collect();
        SpotifyPlaylist {
            id: "sp_test_id".into(),
            title: "Road Trip".into(),
            author: "Tester".into(),
            artwork: Some("https://example.com/cover.jpg".into()),
            tracks,
        }
    }

    fn sample_spotify_playlist() -> SpotifyPlaylist {
        sample_playlist_with_tracks(2)
    }

    fn test_track_event(index: usize, id: &str) -> ImportProgressEvent {
        let t = LocalTrack::new(id, "Song", "Artist", MatchConfidence::High);
        ImportProgressEvent::Track {
            index,
            resolution: TrackResolution::Matched { track: t.clone(), alternatives: vec![t] },
        }
    }

    fn test_progress_event(done: usize, total: usize) -> ImportProgressEvent {
        ImportProgressEvent::Progress { done, total, high: done, review: 0, unresolved: 0 }
    }

    fn test_unresolved_event(index: usize) -> ImportProgressEvent {
        ImportProgressEvent::Track {
            index,
            resolution: TrackResolution::Unresolved {
                title: "Song".into(),
                artist: "Artist".into(),
                reason: "not found".into(),
            },
        }
    }

    fn test_finished_event() -> ImportProgressEvent {
        ImportProgressEvent::Finished {
            playlist: LocalPlaylist::new("local:sp_test_id", "Road Trip"),
        }
    }

    fn test_cancelled_event() -> ImportProgressEvent {
        ImportProgressEvent::Cancelled {
            playlist: LocalPlaylist::new("local:cancelled", "Cancelled"),
        }
    }

    fn test_track_with_alt(id: &str, alt_id: &str) -> ImportProgressEvent {
        let t = LocalTrack::new(id, "Song", "Artist", MatchConfidence::High);
        let alt = LocalTrack::new(alt_id, "Alt", "Artist", MatchConfidence::Medium);
        ImportProgressEvent::Track {
            index: 0,
            resolution: TrackResolution::Matched { track: t.clone(), alternatives: vec![t, alt] },
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
        assert_eq!(session.progress(), (0, 0));
    }

    #[test]
    fn test_start_loading_transitions_stage_and_clears_state() {
        let mut session = ImportSession::new();
        session.fail("prior error");
        session.request_cancel();
        assert!(session.cancel_flag().load(Ordering::Relaxed));

        assert_eq!(session.start_loading(), 1);
        assert_eq!(session.stage, ImportStage::LoadingSource);
        assert!(session.status.is_empty() && !session.cancel_flag().load(Ordering::Relaxed));
    }

    #[test]
    fn test_start_loading_increments_generation() {
        let mut session = ImportSession::new();
        assert_eq!(session.start_loading(), 1);
        assert_eq!(session.start_loading(), 2);
    }

    #[test]
    fn test_accepts_load_result_generation_and_stage_checks() {
        let mut session = ImportSession::new();
        let gen1 = session.start_loading();
        assert!(session.accepts_load_result(gen1) && !session.accepts_load_result(gen1 + 1));

        let gen2 = session.start_loading();
        assert!(!session.accepts_load_result(gen1) && session.accepts_load_result(gen2));

        session.reset_to_idle();
        assert!(!session.accepts_load_result(gen2));
    }

    #[test]
    fn test_is_busy_per_stage() {
        let mut session = ImportSession::new();
        assert_eq!(session.stage, ImportStage::Idle);
        assert!(!session.is_busy());

        session.start_loading();
        assert_eq!(session.stage, ImportStage::LoadingSource);
        assert!(session.is_busy());

        let (_tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);
        assert_eq!(session.stage, ImportStage::Matching);
        assert!(session.is_busy());

        session.stage = ImportStage::Cancelling;
        assert!(session.is_busy());

        session.fail("prior error");
        assert_eq!(session.stage, ImportStage::Failed);
        assert!(!session.is_busy());

        session.stage = ImportStage::Review;
        assert!(!session.is_busy());
    }

    #[test]
    fn test_reset_to_idle_sets_cancel_flag_and_clears_state() {
        let mut session = ImportSession::new();
        session.start_loading();
        session.status = "in progress".into();
        session.picker_row = Some(1);
        session.progress_done = 3;
        session.progress_total = 10;
        assert!(!session.cancel_flag().load(Ordering::Relaxed));

        session.reset_to_idle();
        assert_eq!(session.stage, ImportStage::Idle);
        assert!(session.status.is_empty());
        assert_eq!(session.picker_row, None);
        assert_eq!(session.progress(), (0, 0));
        assert!(session.cancel_flag().load(Ordering::Relaxed));
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
        assert_eq!(session.stage, ImportStage::Cancelling);
        assert_eq!(session.status, "Import cancelled");
    }

    #[test]
    fn test_drain_empty_and_disconnected() {
        let mut session = ImportSession::new();
        assert_eq!(session.drain().applied, 0);

        let (tx, rx) = mpsc::channel::<ImportProgressEvent>();
        session.begin_matching(sample_spotify_playlist(), rx);
        drop(tx);
        assert_eq!(session.drain().applied, 0);
    }

    #[test]
    fn test_drain_applies_track_progress_and_finished_events() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        tx.send(ImportProgressEvent::Started { total: 2 }).unwrap();
        tx.send(test_track_event(0, "v1")).unwrap();
        tx.send(test_progress_event(1, 2)).unwrap();
        tx.send(test_finished_event()).unwrap();

        let outcome = session.drain();
        assert_eq!(outcome.applied, 4);
        assert!(outcome.finished);
        assert!(!outcome.cancelled);
        assert_eq!(session.stage, ImportStage::Review);
        assert_eq!(session.status, "1 / 2");
        assert_eq!(session.progress(), (1, 2));

        let rev = session.review.as_ref().expect("review created");
        assert_eq!(rev.rows.len(), 1);
        assert_eq!(rev.rows[0].source_title, "Song 1");
        assert_eq!(session.counts().high, 1);
    }

    #[test]
    fn test_cancel_returns_to_idle_without_review() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        session.request_cancel();
        assert_eq!(session.stage, ImportStage::Cancelling);
        assert_eq!(session.status, "Import cancelled");
        assert!(session.is_busy());

        tx.send(test_cancelled_event()).unwrap();

        let outcome = session.drain();
        assert_eq!(session.stage, ImportStage::Idle);
        assert!(session.review.is_none());
        assert_eq!(session.counts().total, 0);
        assert!(outcome.cancelled);
    }

    #[test]
    fn test_save_impossible_after_cancel() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);
        session.request_cancel();
        tx.send(test_cancelled_event()).unwrap();
        session.drain();

        let store = MemoryPlaylistStore::new();
        let err = session.save(&store).unwrap_err();
        assert!(err.to_string().contains("no import review"));
    }

    #[test]
    fn test_save_requires_complete_review() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);
        tx.send(ImportProgressEvent::Started { total: 2 }).unwrap();
        tx.send(test_track_event(0, "v1")).unwrap();
        tx.send(test_finished_event()).unwrap();
        let outcome = session.drain();
        assert!(outcome.finished);
        assert_eq!(session.stage, ImportStage::Review);
        assert_eq!(session.review.as_ref().unwrap().rows.len(), 1);

        let store = MemoryPlaylistStore::new();
        let err = session.save(&store).unwrap_err();
        assert!(err.to_string().contains("incomplete"));
    }

    #[test]
    fn test_progress_modal_reports_cancelling() {
        let mut session = ImportSession::new();
        let (_tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);

        session.request_cancel();
        assert_eq!(session.stage, ImportStage::Cancelling);
        assert_eq!(session.status, "Import cancelled");
        assert!(session.is_busy());
        assert!(!session.is_running());
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

        tx.send(test_track_with_alt("v1", "v2")).unwrap();
        tx.send(test_finished_event()).unwrap();
        session.drain();

        assert!(session.open_picker(0) && !session.open_picker(10));
        assert_eq!(session.picker_row, Some(0));

        assert!(session.select_row(0) && !session.select_row(99));

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
        tx.send(test_unresolved_event(0)).unwrap();
        tx.send(test_unresolved_event(1)).unwrap();
        tx.send(test_finished_event()).unwrap();
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

    #[test]
    fn first_of_ten_shows_ten_percent() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_playlist_with_tracks(10), rx);
        tx.send(test_track_event(0, "v0")).unwrap();
        tx.send(test_progress_event(1, 10)).unwrap();
        let outcome = session.drain();
        assert_eq!(outcome.applied, 2);
        assert_eq!(session.progress(), (1, 10));
    }

    #[test]
    fn progress_is_monotonic() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_playlist_with_tracks(10), rx);
        tx.send(test_progress_event(3, 10)).unwrap();
        tx.send(test_progress_event(1, 10)).unwrap();
        session.drain();
        assert_eq!(session.progress().0, 3);
        assert_eq!(session.progress(), (3, 10));
    }

    #[test]
    fn placeholder_not_counted_done() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_playlist_with_tracks(4), rx);
        tx.send(test_track_event(3, "v3")).unwrap();
        tx.send(test_progress_event(1, 4)).unwrap();
        session.drain();
        assert_eq!(session.progress(), (1, 4));
        let review = session.review.as_ref().expect("review created");
        assert_eq!(review.rows.len(), 4);
    }

    #[test]
    fn test_started_without_source_sets_progress_total() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.receiver = Some(rx);
        tx.send(ImportProgressEvent::Started { total: 42 }).unwrap();
        session.drain();
        assert_eq!(session.progress(), (0, 42));
    }

    #[test]
    fn late_progress_does_not_hide_cancel_status() {
        let mut session = ImportSession::new();
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_playlist_with_tracks(10), rx);
        session.request_cancel();
        assert_eq!(session.stage, ImportStage::Cancelling);
        assert_eq!(session.status, "Import cancelled");

        tx.send(test_progress_event(2, 10)).unwrap();
        session.drain();

        assert_eq!(session.status, "Import cancelled");
        assert_eq!(session.progress().0, 2);
    }

    #[test]
    fn test_session_options_defaults_loading_and_reset_lifecycle() {
        let mut session = ImportSession::new();
        assert_eq!(session.destination(), ImportDestination::Library);
        assert_eq!(session.name_override, None);
        assert!(session.resolved_tracks().is_empty() && session.artwork().is_none());

        session.set_options(ImportDestination::Queue, Some("Custom Title".into()));
        assert_eq!(session.destination(), ImportDestination::Queue);
        assert_eq!(session.name_override.as_deref(), Some("Custom Title"));

        session.start_loading();
        assert_eq!(session.destination(), ImportDestination::Queue);
        assert_eq!(session.name_override.as_deref(), Some("Custom Title"));

        session.reset_to_idle();
        assert_eq!(session.destination(), ImportDestination::Library);
        assert_eq!(session.name_override, None);
    }

    #[test]
    fn test_save_writes_overridden_title_through_memory_store() {
        let store = MemoryPlaylistStore::new();
        let mut session = ImportSession::new();
        session.set_options(ImportDestination::Library, Some("Renamed Road Trip".into()));
        let (tx, rx) = mpsc::channel();
        session.begin_matching(sample_spotify_playlist(), rx);
        tx.send(test_track_event(0, "v1")).unwrap();
        tx.send(test_track_event(1, "v2")).unwrap();
        tx.send(test_finished_event()).unwrap();
        session.drain();

        let saved = session.save(&store).expect("save succeeds");
        assert_eq!(saved.title, "Renamed Road Trip");
        assert_eq!(store.get("local:sp_test_id").unwrap().title, "Renamed Road Trip");
        assert_eq!(session.resolved_tracks().len(), 2);
        assert_eq!(session.artwork().as_deref(), Some("https://example.com/cover.jpg"));
    }
}
