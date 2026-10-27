//! Skip segment boundaries and persistence for [`AppState`].

use crate::app::AppState;
use crate::domain::{SkipRanges, SkipSegment};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore, SkipStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects an alternative skip store implementation and loads persisted ranges into state.
    pub fn set_skip_store(&mut self, store: Box<dyn SkipStore>) {
        self.skips = store.load();
        self.skip_store = store;
    }

    fn persist_skips(&self) {
        if let Err(err) = self.skip_store.save(&self.skips) {
            tracing::warn!("failed to persist skips: {err:#}");
        }
    }

    /// Captures the current audio playback position as the segment start.
    pub fn mark_segment_start(&mut self) {
        let pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        self.mark_segment_start_at(pos);
    }

    /// Sets the segment start boundary explicitly (for testing and exact positioning).
    pub fn mark_segment_start_at(&mut self, pos_secs: u32) {
        self.marked_start_secs = Some(pos_secs);
        self.skip_error = None;
    }

    /// Captures current playback position as segment end and stores the range when valid.
    pub fn mark_segment_end(&mut self) -> bool {
        let pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        self.mark_segment_end_at(pos)
    }

    /// Sets segment end boundary explicitly and stores range when valid.
    pub fn mark_segment_end_at(&mut self, pos_secs: u32) -> bool {
        let Some(track) = self.queue.current() else {
            self.skip_error = Some("No active track".to_string());
            return false;
        };
        let Some(start) = self.marked_start_secs else {
            self.skip_error = Some("Mark start first".to_string());
            return false;
        };
        let Ok(segment) = SkipSegment::new(start, pos_secs) else {
            self.skip_error = Some("End must be greater than start".to_string());
            return false;
        };
        let track_id = track.id.clone();
        self.skips.entry(track_id).or_default().insert(segment);
        self.persist_skips();
        self.marked_start_secs = None;
        self.skip_error = None;
        true
    }

    /// Deletes a stored range at `index` for `track_id`, persisting changes.
    pub fn delete_segment(&mut self, track_id: &str, index: usize) -> bool {
        let Some(ranges) = self.skips.get_mut(track_id) else {
            return false;
        };
        let removed = ranges.remove(index).is_some();
        if removed {
            self.persist_skips();
        }
        removed
    }

    /// Returns stored skip ranges for the currently playing track.
    pub fn current_skip_ranges(&self) -> Option<&SkipRanges> {
        let track = self.queue.current()?;
        self.skips.get(&track.id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::{SkipSegment, Track};
    use crate::infra::MemorySkipStore;

    #[test]
    fn test_mark_segment_flow_validation_and_errors() {
        let mut state = wired();

        // 1. Mark end without active track
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("No active track"));

        let track = Track::new("track-flow", "Flow Song", "Artist");
        state.queue.push(track);

        // 2. Mark end without marking start first
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("Mark start first"));

        // 3. Mark start at 20, then mark end at 10 (end < start)
        state.mark_segment_start_at(20);
        assert_eq!(state.marked_start_secs, Some(20));
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("End must be greater than start"));
        assert_eq!(state.marked_start_secs, Some(20));

        // 4. Mark end at 20 (end == start)
        assert!(!state.mark_segment_end_at(20));
        assert_eq!(state.skip_error.as_deref(), Some("End must be greater than start"));

        // 5. Mark end at 35 (valid: 20 < 35)
        assert!(state.mark_segment_end_at(35));
        assert_eq!(state.marked_start_secs, None);
        assert_eq!(state.skip_error, None);

        let stored = state.current_skip_ranges().expect("ranges present");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored.segments(), &[SkipSegment { start_secs: 20, end_secs: 35 }]);
    }

    #[test]
    fn test_delete_segment_and_store_persistence() {
        let mut state = wired();
        let store = MemorySkipStore::new();
        state.set_skip_store(Box::new(store.clone()));

        let track = Track::new("track-del", "Delete Song", "Artist");
        state.queue.push(track);

        state.mark_segment_start_at(5);
        state.mark_segment_end_at(15);
        state.mark_segment_start_at(30);
        state.mark_segment_end_at(40);

        assert_eq!(state.current_skip_ranges().unwrap().len(), 2);
        assert_eq!(store.load().get("track-del").unwrap().len(), 2);

        // Delete invalid index returns false
        assert!(!state.delete_segment("track-del", 5));

        // Delete first segment
        assert!(state.delete_segment("track-del", 0));
        assert_eq!(state.current_skip_ranges().unwrap().len(), 1);
        assert_eq!(
            state.current_skip_ranges().unwrap().segments(),
            &[SkipSegment { start_secs: 30, end_secs: 40 }]
        );
        assert_eq!(store.load().get("track-del").unwrap().len(), 1);

        // Delete remaining segment
        assert!(state.delete_segment("track-del", 0));
        assert_eq!(state.current_skip_ranges().unwrap().len(), 0);
        assert_eq!(store.load().get("track-del").unwrap().len(), 0);
    }

    #[test]
    fn test_mark_segment_captures_audio_position() {
        let mut state = wired();
        let track = Track::new("track-audio", "Audio Pos Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.audio.seek(12);
        state.mark_segment_start();
        assert_eq!(state.marked_start_secs, Some(12));

        state.audio.seek(24);
        assert!(state.mark_segment_end());
        assert_eq!(state.marked_start_secs, None);

        let stored = state.current_skip_ranges().expect("stored ranges");
        assert_eq!(stored.segments(), &[SkipSegment { start_secs: 12, end_secs: 24 }]);
    }
}
