//! Listening history models, play record serialization, and duration clamping.
//!
//! Hexagonal invariant: pure domain logic only; no I/O, no wall-clock timing, no randomness.

use serde::{Deserialize, Serialize};

use super::Track;

/// Minimum listened duration in seconds required to record a play.
pub const MIN_RECORDED_PLAY_SECS: u64 = 30;

/// Persisted record of a completed or abandoned track playback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayRecord {
    /// Stable catalog identifier or video id.
    pub video_id: String,
    /// Track title.
    pub title: String,
    /// Artist display name.
    pub artist: String,
    /// Optional album name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Unix epoch timestamp in seconds when playback started.
    pub started_at_unix: u64,
    /// Effective listened time in seconds clamped to track duration.
    pub listened_secs: u64,
    /// Optional track duration in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u32>,
}

/// Clamps elapsed wall-clock listening time to track duration when known.
pub fn listened_secs(elapsed_secs: u64, duration_secs: Option<u32>) -> u64 {
    match duration_secs {
        Some(dur) => elapsed_secs.min(u64::from(dur)),
        None => elapsed_secs,
    }
}

/// Builds a play record if the clamped listened time meets the minimum recording threshold.
pub fn record_for_play(
    track: &Track,
    started_at_unix: u64,
    elapsed_secs: u64,
) -> Option<PlayRecord> {
    let listened = listened_secs(elapsed_secs, track.duration_secs);
    if listened < MIN_RECORDED_PLAY_SECS {
        return None;
    }
    Some(PlayRecord {
        video_id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        started_at_unix,
        listened_secs: listened,
        duration_secs: track.duration_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_for_play_below_threshold_returns_none() {
        let track = Track::new("vid-1", "Title", "Artist").with_duration(200);
        assert!(record_for_play(&track, 1_000, 29).is_none());
        assert!(record_for_play(&track, 1_000, 0).is_none());
    }

    #[test]
    fn test_record_for_play_exactly_threshold_returns_some() {
        let track = Track::new("vid-1", "Title", "Artist").with_duration(200);
        let record = record_for_play(&track, 1_000, 30).expect("recorded at exactly 30 seconds");
        assert_eq!(record.video_id, "vid-1");
        assert_eq!(record.title, "Title");
        assert_eq!(record.artist, "Artist");
        assert_eq!(record.album, None);
        assert_eq!(record.started_at_unix, 1_000);
        assert_eq!(record.listened_secs, 30);
        assert_eq!(record.duration_secs, Some(200));
    }

    #[test]
    fn test_listened_secs_clamps_above_duration() {
        assert_eq!(listened_secs(300, Some(180)), 180);
        let track = Track::new("vid-1", "Title", "Artist").with_duration(180);
        let record = record_for_play(&track, 1_000, 300).expect("recorded");
        assert_eq!(record.listened_secs, 180);
    }

    #[test]
    fn test_listened_secs_unknown_duration_retains_elapsed() {
        assert_eq!(listened_secs(45, None), 45);
        assert_eq!(listened_secs(300, None), 300);
        let track = Track::new("vid-1", "Title", "Artist");
        let record = record_for_play(&track, 1_000, 45).expect("recorded");
        assert_eq!(record.listened_secs, 45);
        assert_eq!(record.duration_secs, None);
    }

    #[test]
    fn test_play_record_serde_round_trip_omits_none_fields() {
        let record = PlayRecord {
            video_id: "vid-1".to_string(),
            title: "Title".to_string(),
            artist: "Artist".to_string(),
            album: None,
            started_at_unix: 1_700_000_000,
            listened_secs: 42,
            duration_secs: None,
        };

        let json = serde_json::to_string(&record).expect("serialization succeeds");
        assert!(!json.contains("album"), "absent album must be omitted from JSON: {json}");
        assert!(
            !json.contains("duration_secs"),
            "absent duration_secs must be omitted from JSON: {json}"
        );

        let deserialized: PlayRecord =
            serde_json::from_str(&json).expect("deserialization succeeds");
        assert_eq!(deserialized, record);
    }

    #[test]
    fn test_play_record_serde_round_trip_full_fields() {
        let record = PlayRecord {
            video_id: "vid-2".to_string(),
            title: "Full Title".to_string(),
            artist: "Full Artist".to_string(),
            album: Some("Great Album".to_string()),
            started_at_unix: 1_700_000_500,
            listened_secs: 150,
            duration_secs: Some(200),
        };

        let json = serde_json::to_string(&record).expect("serialization succeeds");
        assert!(json.contains("\"album\":\"Great Album\""));
        assert!(json.contains("\"duration_secs\":200"));

        let deserialized: PlayRecord =
            serde_json::from_str(&json).expect("deserialization succeeds");
        assert_eq!(deserialized, record);
    }
}
