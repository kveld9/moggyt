//! Playback history recording on SlintAppState.
//!
//! Hexagonal invariant: non-blocking persistence of play events via the PlayLogStore port.

use crate::domain::{PlayRecord, RECENT_PLAY_WINDOW, record_for_play};
use crate::infra::PlayLogStore;
use crate::slint_shell::playback_control::StartedPlay;
use crate::slint_shell::state::SlintAppState;

/// Returns the video ids of the most recent plays, newest first and deduplicated.
///
/// A read failure degrades to an empty list: smart shuffle then behaves as a
/// plain shuffle instead of failing playback.
pub(crate) fn recent_video_ids(store: &dyn PlayLogStore) -> Vec<String> {
    let Ok(records) = store.load() else {
        tracing::warn!("failed to load play log for smart shuffle ordering");
        return Vec::new();
    };
    newest_unique_video_ids(&records)
}

fn newest_unique_video_ids(records: &[PlayRecord]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for record in records.iter().rev().take(RECENT_PLAY_WINDOW) {
        if !ids.contains(&record.video_id) {
            ids.push(record.video_id.clone());
        }
    }
    ids
}

impl SlintAppState {
    /// Records a finished or abandoned track play when the listened threshold is met.
    // NOTE(clippy): tracing macro expansion inflates cognitive complexity.
    #[allow(clippy::cognitive_complexity)]
    pub(crate) fn record_play(&self, started: &StartedPlay, elapsed_secs: u64) {
        let Some(record) = record_for_play(&started.track, started.unix_secs, elapsed_secs) else {
            return;
        };
        if let Err(e) = self.play_log_store.append(&record) {
            tracing::warn!(error = %e, "failed to append play log entry");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::domain::Track;
    use crate::infra::{MemoryPlayLogStore, PlayLogStore};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_record_play_qualifying_track_persists_record() {
        let memory_store = MemoryPlayLogStore::new();
        let mut state = test_wired();
        state.play_log_store = Box::new(memory_store.clone());

        let track = Track::new("video-abc", "Midnight City", "M83")
            .with_album("Hurry Up, We're Dreaming")
            .with_duration(200);
        let started = StartedPlay {
            unix_secs: 1_700_000_123,
            track,
            listened: Duration::ZERO,
            resumed_at: None,
        };

        state.record_play(&started, 150);

        let records = memory_store.load().expect("load succeeds");
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.video_id, "video-abc");
        assert_eq!(record.title, "Midnight City");
        assert_eq!(record.artist, "M83");
        assert_eq!(record.album.as_deref(), Some("Hurry Up, We're Dreaming"));
        assert_eq!(record.started_at_unix, 1_700_000_123);
        assert_eq!(record.listened_secs, 150);
        assert_eq!(record.duration_secs, Some(200));
    }

    #[test]
    fn test_record_play_below_threshold_is_not_persisted() {
        let memory_store = MemoryPlayLogStore::new();
        let mut state = test_wired();
        state.play_log_store = Box::new(memory_store.clone());

        let track = Track::new("video-xyz", "Short Clip", "Various").with_duration(200);
        let started = StartedPlay {
            unix_secs: 1_700_000_500,
            track,
            listened: Duration::ZERO,
            resumed_at: None,
        };

        state.record_play(&started, 10);

        let records = memory_store.load().expect("load succeeds");
        assert!(records.is_empty(), "plays under 30 seconds must not be recorded");
    }

    fn play_record(video_id: &str) -> PlayRecord {
        PlayRecord {
            video_id: video_id.to_string(),
            title: format!("Title {video_id}"),
            artist: "Artist".to_string(),
            album: None,
            started_at_unix: 1_700_000_000,
            listened_secs: 120,
            duration_secs: Some(200),
        }
    }

    #[test]
    fn test_recent_video_ids_returns_newest_first_and_deduplicated() {
        let store = MemoryPlayLogStore::new();
        for id in ["old", "dup", "dup", "newest"] {
            store.append(&play_record(id)).expect("append succeeds");
        }
        let ids = recent_video_ids(&store);
        assert_eq!(ids, vec!["newest".to_string(), "dup".to_string(), "old".to_string()]);
    }

    #[test]
    fn test_recent_video_ids_degrades_to_empty_on_store_error() {
        struct FailingStore;
        impl PlayLogStore for FailingStore {
            fn load(&self) -> anyhow::Result<Vec<PlayRecord>> {
                Err(anyhow::anyhow!("disk unavailable"))
            }
            fn append(&self, _record: &PlayRecord) -> anyhow::Result<()> {
                Ok(())
            }
        }
        assert!(recent_video_ids(&FailingStore).is_empty());
    }
}
