//! Playback progression: end-of-track advance, crossfade, and sample-continuous hand-off.
//!
//! Gapless hand-off decodes the next queue track while the active one is still playing,
//! appends it behind the live sink, and advances the queue cursor only when the backend
//! reports the sample boundary. Crossfade keeps priority when it is configured.

use crate::domain::{PlaybackPhase, Track, should_arm};
use crate::infra::{AudioBackend, Catalog};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::observe_playback_phase;

/// UI-side latch for the sample-continuous hand-off.
///
/// `armed_for` marks the queue track already dispatched for preloading; `pending` holds the
/// preloaded result until the audio backend crosses the sample boundary.
#[derive(Default)]
pub struct GaplessUi {
    armed_for: Option<String>,
    pending: Option<PlayTrackResult>,
}

impl GaplessUi {
    /// Whether no preload is armed and no preloaded result is waiting for the boundary.
    pub fn is_free(&self) -> bool {
        self.armed_for.is_none() && self.pending.is_none()
    }

    /// Clears the arm marker and any pending preloaded result.
    pub fn reset(&mut self) {
        self.armed_for = None;
        self.pending = None;
    }

    /// Tracks the queue id dispatched for preloading.
    pub fn mark_armed(&mut self, track_id: &str) {
        self.armed_for = Some(track_id.to_owned());
    }

    fn matches_arm(&self, track_id: &str) -> bool {
        self.armed_for.as_deref() == Some(track_id)
    }

    fn store(&mut self, result: PlayTrackResult) {
        self.pending = Some(result);
    }

    fn take(&mut self) -> Option<PlayTrackResult> {
        self.pending.take()
    }
}

impl SlintAppState {
    fn enqueue_radio_recommendations(&mut self, recs: Vec<Track>) {
        let worker_tx = self.worker_tx.clone();
        let generation = self.play_generation;
        for track in recs {
            if !self.queue.tracks().iter().any(|t| t.id == track.id) {
                if let Some(ref tx) = worker_tx {
                    request_queue_track_cover(
                        track.artwork.as_deref(),
                        generation,
                        &self.search_covers,
                        tx,
                    );
                }
                self.queue.push_autoplay(track);
            }
        }
    }

    fn fetch_autoplay_radio(&self, current_id: &str) -> Option<Vec<Track>> {
        let Ok(catalog) = self.catalog.lock() else {
            tracing::warn!("failed to fetch autoplay radio: catalog lock poisoned");
            return None;
        };
        match catalog.radio(current_id) {
            Ok(tracks) => Some(tracks),
            Err(err) => {
                tracing::warn!("failed to fetch autoplay radio: {err}");
                None
            }
        }
    }

    pub(crate) fn append_autoplay_radio(&mut self, current_id: &str) {
        let Some(recs) = self.fetch_autoplay_radio(current_id) else {
            return;
        };
        let filtered = if self.songs_only { crate::domain::retain_songs(recs) } else { recs };
        self.enqueue_radio_recommendations(filtered);
    }

    fn track_remaining_secs(&self, audio: &dyn AudioBackend) -> Option<u64> {
        let dur = self.queue.current()?.duration_secs? as u64;
        let pos = audio.position_secs();
        (dur > pos).then(|| dur - pos)
    }

    fn can_crossfade_track(&self, track_id: &str) -> bool {
        if self.last_crossfaded_track.as_deref() == Some(track_id) {
            return false;
        }
        self.queue.peek_next().is_some() || self.autoplay
    }

    fn crossfade_candidate(&self, audio: &dyn AudioBackend) -> Option<(String, bool)> {
        let remaining = self.track_remaining_secs(audio)?;
        if remaining > self.crossfade_seconds as u64 {
            return None;
        }
        let current_id = &self.queue.current()?.id;
        if !self.can_crossfade_track(current_id) {
            return None;
        }
        Some((current_id.clone(), self.queue.peek_next().is_some()))
    }

    fn handle_playing_crossfade(&self, audio: &dyn AudioBackend) -> Option<(String, bool)> {
        if self.crossfade_seconds == 0 {
            return None;
        }
        self.crossfade_candidate(audio)
    }

    fn execute_crossfade(&mut self, current_id: String, has_next: bool) {
        self.last_crossfaded_track = Some(current_id.clone());
        if !has_next && self.autoplay {
            self.append_autoplay_radio(&current_id);
        }
        self.next_track();
    }

    fn advance_ended_playback(&mut self) {
        if self.playback_phase != PlaybackPhase::Ended {
            return;
        }
        if self.queue.peek_next().is_some() {
            self.next_track();
            return;
        }
        if !self.autoplay {
            self.maybe_scrobble_current();
            return;
        }
        let Some(current) = self.queue.current() else {
            return;
        };
        let current_id = current.id.clone();
        self.append_autoplay_radio(&current_id);
        self.queue.advance();
        self.persist_queue();
        self.request_current_play();
    }

    /// Advances playback at a track end or sample boundary, returning metadata to apply.
    ///
    /// The returned result belongs to a gapless preload that just became audible; the caller
    /// applies it and re-syncs the window. Pausing never advances playback.
    pub fn continue_autoplay_if_ended(&mut self) -> Option<PlayTrackResult> {
        observe_playback_phase(self);
        if self.queue.is_empty() {
            return None;
        }
        if let Some(result) = self.adopt_started_gapless() {
            return Some(result);
        }
        if self.playback_phase == PlaybackPhase::Ended {
            self.advance_ended_playback();
            return None;
        }
        if self.playback_phase != PlaybackPhase::Playing {
            return None;
        }
        let candidate =
            self.audio.try_lock().ok().and_then(|audio| self.handle_playing_crossfade(&**audio));
        if let Some((id, has_next)) = candidate {
            self.execute_crossfade(id, has_next);
            return None;
        }
        self.arm_gapless_if_due();
        None
    }

    fn sleep_timer_stops_after_track(&self) -> bool {
        self.sleep_timer.is_armed()
            && self.sleep_timer.preset() == crate::domain::TimerPreset::StopAfterTrack
    }

    fn remaining_secs_locked(&self) -> Option<u64> {
        let audio = self.audio.try_lock().ok()?;
        if audio.is_empty() {
            return None;
        }
        self.track_remaining_secs(&**audio)
    }

    fn arm_gapless_if_due(&mut self) {
        if !self.gapless.is_free() || self.sleep_timer_stops_after_track() {
            return;
        }
        let remaining = self.remaining_secs_locked();
        let has_next = self.queue.peek_next().is_some();
        if !should_arm(self.playback_phase, remaining, self.crossfade_seconds, has_next) {
            return;
        }
        self.dispatch_gapless_preload();
    }

    fn dispatch_gapless_preload(&mut self) {
        let Some(next_id) = self.queue.peek_next().map(|track| track.id.clone()) else {
            return;
        };
        let Some(request) = self.begin_gapless_request() else {
            return;
        };
        self.gapless.mark_armed(&next_id);
        self.send_play_request(request);
    }

    fn gapless_matches_queue(&self, started_id: &str, result: &PlayTrackResult) -> bool {
        result.track_id == started_id
            && self.queue.peek_next().map(|track| track.id.as_str()) == Some(started_id)
    }

    fn adopt_started_gapless(&mut self) -> Option<PlayTrackResult> {
        if self.playback_phase != PlaybackPhase::Playing || self.gapless.pending.is_none() {
            return None;
        }
        let started = {
            let mut audio = self.audio.try_lock().ok()?;
            audio.take_started_gapless()?
        };
        let result = self.gapless.take()?;
        self.gapless.reset();
        if !self.gapless_matches_queue(&started.track_id, &result) {
            return None;
        }
        self.on_track_changing_sleep_timer(&started.track_id);
        self.maybe_scrobble_current();
        self.queue.advance();
        self.persist_queue();
        self.play_generation = self.play_generation.wrapping_add(1);
        Some(result)
    }

    /// Stores a preloaded gapless result until the backend crosses the sample boundary.
    pub fn on_gapless_preload_result(&mut self, result: PlayTrackResult) {
        if !result.gapless {
            return;
        }
        if !result.success || !self.gapless.matches_arm(&result.track_id) {
            self.gapless.reset();
            return;
        }
        self.gapless.store(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::slint_shell::platform::PresenceSnapshot;
    use crate::slint_shell::test_support::test_wired;

    fn preloaded_result(track_id: &str, success: bool) -> PlayTrackResult {
        PlayTrackResult {
            generation: 0,
            track_id: track_id.to_string(),
            title: "Next".to_string(),
            artist: "Artist".to_string(),
            success,
            lyrics_lines: Vec::new(),
            cover_data: None,
            presence: PresenceSnapshot::new("Next", "Artist", success),
            gapless: true,
        }
    }

    fn playing_state() -> SlintAppState {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist").with_duration(100));
        state.queue.push(Track::new("t2", "Track 2", "Artist").with_duration(120));
        state.play_track_at_index(0);
        state
    }

    #[test]
    fn gapless_arms_inside_the_verge_window() {
        let mut state = playing_state();
        state.audio.lock().unwrap().seek(97);

        state.continue_autoplay_if_ended();

        assert_eq!(state.queue.cursor(), 0, "the cursor advances only at the boundary");
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t2"));
        assert!(!state.gapless.is_free());
    }

    #[test]
    fn gapless_does_not_arm_outside_the_verge_window() {
        let mut state = playing_state();
        state.audio.lock().unwrap().seek(80);

        state.continue_autoplay_if_ended();

        assert!(state.gapless.is_free());
        assert_eq!(state.queue.cursor(), 0);
    }

    #[test]
    fn gapless_does_not_arm_when_crossfade_is_configured() {
        let mut state = playing_state();
        state.crossfade_seconds = 4;
        state.audio.lock().unwrap().seek(97);

        state.continue_autoplay_if_ended();

        assert!(state.gapless.is_free());
        assert_eq!(state.queue.cursor(), 1, "crossfade owns the transition");
    }

    #[test]
    fn gapless_does_not_arm_on_the_last_track() {
        let mut state = test_wired();
        state.queue.push(Track::new("only", "Only", "Artist").with_duration(100));
        state.play_track_at_index(0);
        state.audio.lock().unwrap().seek(99);

        state.continue_autoplay_if_ended();

        assert!(state.gapless.is_free());
    }

    #[test]
    fn adopting_the_started_track_advances_once() {
        let mut state = playing_state();
        state.gapless.mark_armed("t2");
        state.on_gapless_preload_result(preloaded_result("t2", true));
        {
            let mut audio = state.audio.lock().unwrap();
            audio.play_gapless("t2").expect("preload armed");
            audio.finish_gapless();
        }

        let result = state.continue_autoplay_if_ended();
        let result = result.expect("preloaded track became audible");
        assert_eq!(result.track_id, "t2");
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.queue_store.load().cursor(), 1);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
        assert!(state.gapless.is_free());

        assert!(state.continue_autoplay_if_ended().is_none(), "the hand-off is single use");
        assert_eq!(state.queue.cursor(), 1);
    }

    #[test]
    fn adoption_waits_for_the_sample_boundary() {
        let mut state = playing_state();
        state.gapless.mark_armed("t2");
        state.on_gapless_preload_result(preloaded_result("t2", true));

        assert!(state.continue_autoplay_if_ended().is_none());
        assert_eq!(state.queue.cursor(), 0);
    }

    #[test]
    fn failed_preload_clears_the_latch() {
        let mut state = playing_state();
        state.gapless.mark_armed("t2");
        state.on_gapless_preload_result(preloaded_result("t2", false));
        assert!(state.gapless.is_free());
    }

    #[test]
    fn preload_does_not_arm_while_the_sleep_timer_stops_after_the_track() {
        let mut state = playing_state();
        state.audio.lock().unwrap().seek(99);
        state
            .sleep_timer
            .set_preset_with_track(crate::domain::TimerPreset::StopAfterTrack, Some("t1"));

        state.continue_autoplay_if_ended();

        assert!(state.gapless.is_free());
    }

    #[test]
    fn paused_playback_never_arms() {
        let mut state = playing_state();
        state.audio.lock().unwrap().seek(99);
        state.toggle_play();
        assert_eq!(state.playback_phase, PlaybackPhase::Paused);

        state.continue_autoplay_if_ended();

        assert!(state.gapless.is_free());
    }

    #[test]
    fn radio_recommendations_land_as_autoplay_and_do_not_change_visible_len() {
        let mut state = test_wired();
        state.queue.push(Track::new("c1", "Context", "Artist"));
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.visible_len(), 1);

        let recs =
            vec![Track::new("r1", "Radio 1", "Artist"), Track::new("r2", "Radio 2", "Artist")];
        state.enqueue_radio_recommendations(recs);

        assert_eq!(state.queue.len(), 3);
        assert_eq!(state.queue.visible_len(), 1);
        assert!(!state.queue.is_autoplay_at(0));
        assert!(state.queue.is_autoplay_at(1));
        assert!(state.queue.is_autoplay_at(2));
    }

    #[test]
    fn test_ended_advance_persists_new_cursor() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist").with_duration(100));
        state.queue.push(Track::new("t2", "Track 2", "Artist").with_duration(120));
        state.play_track_at_index(0);
        assert_eq!(state.queue_store.load().cursor(), 0);

        state.set_playback_phase(PlaybackPhase::Ended);
        state.continue_autoplay_if_ended();

        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.queue_store.load().cursor(), 1);
        assert_eq!(state.queue_store.load().current().map(|t| t.id.as_str()), Some("t2"));
    }
}
