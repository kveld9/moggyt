//! Wrapped-style listening statistics aggregation and time-window filtering.
//!
//! Hexagonal invariant: pure domain logic only; no I/O, no wall-clock timing, no randomness.

use std::collections::HashMap;

use super::PlayRecord;

/// Default upper bound for top artists and top tracks rankings.
pub const STATS_TOP_LIMIT: usize = 5;

/// Number of seconds in a standard 24-hour day.
pub const SECS_PER_DAY: u64 = 86_400;

/// Rolling time window for computing listening statistics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StatsPeriod {
    /// Rolling window spanning the last 4 weeks (28 days).
    Last4Weeks,
    /// Rolling window spanning the last 6 months (182 days).
    Last6Months,
    /// All available listening history without time restriction.
    #[default]
    AllTime,
}

impl StatsPeriod {
    /// Maps a 0-based index to a `StatsPeriod`; out-of-range index values default to `AllTime`.
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Last4Weeks,
            1 => Self::Last6Months,
            _ => Self::AllTime,
        }
    }

    /// Converts this period into its 0-based selection index.
    pub fn to_index(self) -> i32 {
        match self {
            Self::Last4Weeks => 0,
            Self::Last6Months => 1,
            Self::AllTime => 2,
        }
    }

    /// Returns the rolling window duration in days, or `None` for all-time stats.
    pub fn window_days(self) -> Option<u64> {
        match self {
            Self::Last4Weeks => Some(28),
            Self::Last6Months => Some(182),
            Self::AllTime => None,
        }
    }

    /// Computes the unix timestamp threshold for this period relative to `now_unix`.
    pub fn since_unix(self, now_unix: u64) -> Option<u64> {
        self.window_days().map(|days| now_unix.saturating_sub(days * SECS_PER_DAY))
    }
}

/// A ranked entry representing a top track or artist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedEntry {
    /// Display name of the ranked item (track title or artist name).
    pub name: String,
    /// Secondary detail (artist display name for tracks, empty string for artists).
    pub detail: String,
    /// Total play count within the evaluated window.
    pub plays: u32,
    /// Cumulative listening duration in seconds within the evaluated window.
    pub listened_secs: u64,
}

/// Summary listening statistics computed from playback records.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListeningStats {
    /// Total number of plays within the evaluated window.
    pub total_plays: u32,
    /// Total duration listened in seconds within the evaluated window.
    pub total_listened_secs: u64,
    /// Number of distinct tracks played within the evaluated window.
    pub unique_tracks: usize,
    /// Number of distinct artists played within the evaluated window.
    pub unique_artists: usize,
    /// Top-ranked artists ordered by plays descending, listened duration, and name.
    pub top_artists: Vec<RankedEntry>,
    /// Top-ranked tracks ordered by plays descending, listened duration, and name.
    pub top_tracks: Vec<RankedEntry>,
}

impl ListeningStats {
    /// Returns `true` if no plays are recorded in this statistical summary.
    pub fn is_empty(&self) -> bool {
        self.total_plays == 0
    }
}

#[derive(Debug)]
struct TrackAggregate {
    name: String,
    detail: String,
    latest_started_at: u64,
    plays: u32,
    listened_secs: u64,
}

impl TrackAggregate {
    fn new(record: &PlayRecord) -> Self {
        Self {
            name: record.title.clone(),
            detail: record.artist.clone(),
            latest_started_at: record.started_at_unix,
            plays: 1,
            listened_secs: record.listened_secs,
        }
    }

    fn absorb(&mut self, record: &PlayRecord) {
        self.plays = self.plays.saturating_add(1);
        self.listened_secs = self.listened_secs.saturating_add(record.listened_secs);
        if record.started_at_unix >= self.latest_started_at {
            self.latest_started_at = record.started_at_unix;
            self.name.clone_from(&record.title);
            self.detail.clone_from(&record.artist);
        }
    }

    fn into_ranked_entry(self) -> RankedEntry {
        RankedEntry {
            name: self.name,
            detail: self.detail,
            plays: self.plays,
            listened_secs: self.listened_secs,
        }
    }
}

#[derive(Debug)]
struct ArtistAggregate {
    name: String,
    plays: u32,
    listened_secs: u64,
}

impl ArtistAggregate {
    fn new(record: &PlayRecord) -> Self {
        Self { name: record.artist.clone(), plays: 1, listened_secs: record.listened_secs }
    }

    fn absorb(&mut self, record: &PlayRecord) {
        self.plays = self.plays.saturating_add(1);
        self.listened_secs = self.listened_secs.saturating_add(record.listened_secs);
    }

    fn into_ranked_entry(self) -> RankedEntry {
        RankedEntry {
            name: self.name,
            detail: String::new(),
            plays: self.plays,
            listened_secs: self.listened_secs,
        }
    }
}

fn compare_ranked_entries(a: &RankedEntry, b: &RankedEntry) -> std::cmp::Ordering {
    b.plays
        .cmp(&a.plays)
        .then_with(|| b.listened_secs.cmp(&a.listened_secs))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.detail.cmp(&b.detail))
}

fn sort_and_truncate(mut entries: Vec<RankedEntry>, limit: usize) -> Vec<RankedEntry> {
    entries.sort_unstable_by(compare_ranked_entries);
    entries.truncate(limit);
    entries
}

fn aggregate_tracks<'a>(
    records: impl Iterator<Item = &'a PlayRecord>,
) -> HashMap<String, TrackAggregate> {
    let mut map = HashMap::<String, TrackAggregate>::new();
    for record in records {
        map.entry(record.video_id.clone())
            .and_modify(|agg| agg.absorb(record))
            .or_insert_with(|| TrackAggregate::new(record));
    }
    map
}

fn aggregate_artists<'a>(
    records: impl Iterator<Item = &'a PlayRecord>,
) -> HashMap<String, ArtistAggregate> {
    let mut map = HashMap::<String, ArtistAggregate>::new();
    for record in records {
        map.entry(record.artist.clone())
            .and_modify(|agg| agg.absorb(record))
            .or_insert_with(|| ArtistAggregate::new(record));
    }
    map
}

/// Computes summary listening statistics and top rankings from playback records.
///
/// Records with `started_at_unix` before `since_unix` are excluded when a cutoff is specified.
/// Top tracks and artists are truncated to `limit`.
pub fn compute_listening_stats(
    records: &[PlayRecord],
    since_unix: Option<u64>,
    limit: usize,
) -> ListeningStats {
    let filtered: Vec<&PlayRecord> = records
        .iter()
        .filter(|record| since_unix.is_none_or(|cutoff| record.started_at_unix >= cutoff))
        .collect();

    let total_plays = u32::try_from(filtered.len()).unwrap_or(u32::MAX);
    let total_listened_secs =
        filtered.iter().fold(0u64, |acc, record| acc.saturating_add(record.listened_secs));

    let track_map = aggregate_tracks(filtered.iter().copied());
    let artist_map = aggregate_artists(filtered.iter().copied());

    let unique_tracks = track_map.len();
    let unique_artists = artist_map.len();

    let top_tracks = sort_and_truncate(
        track_map.into_values().map(TrackAggregate::into_ranked_entry).collect(),
        limit,
    );
    let top_artists = sort_and_truncate(
        artist_map.into_values().map(ArtistAggregate::into_ranked_entry).collect(),
        limit,
    );

    ListeningStats {
        total_plays,
        total_listened_secs,
        unique_tracks,
        unique_artists,
        top_artists,
        top_tracks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(
        video_id: &str,
        title: &str,
        artist: &str,
        started_at_unix: u64,
        listened_secs: u64,
    ) -> PlayRecord {
        PlayRecord {
            video_id: video_id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: None,
            started_at_unix,
            listened_secs,
            duration_secs: None,
        }
    }

    #[test]
    fn test_empty_input_returns_default_stats_and_is_empty() {
        let stats = compute_listening_stats(&[], None, STATS_TOP_LIMIT);
        assert_eq!(stats, ListeningStats::default());
        assert!(stats.is_empty());
    }

    #[test]
    fn test_totals_calculation() {
        let records = vec![
            make_record("v1", "Song 1", "Artist A", 1_000, 40),
            make_record("v1", "Song 1", "Artist A", 1_100, 60),
            make_record("v2", "Song 2", "Artist B", 1_200, 100),
        ];
        let stats = compute_listening_stats(&records, None, STATS_TOP_LIMIT);
        assert_eq!(stats.total_plays, 3);
        assert_eq!(stats.total_listened_secs, 200);
        assert_eq!(stats.unique_tracks, 2);
        assert_eq!(stats.unique_artists, 2);
        assert!(!stats.is_empty());
    }

    #[test]
    fn test_since_unix_filter_boundary() {
        let records = vec![
            make_record("v1", "Old Song", "Artist A", 999, 50),
            make_record("v2", "Boundary Song", "Artist B", 1_000, 60),
            make_record("v3", "New Song", "Artist C", 1_001, 70),
        ];
        let stats = compute_listening_stats(&records, Some(1_000), STATS_TOP_LIMIT);
        assert_eq!(stats.total_plays, 2);
        assert_eq!(stats.total_listened_secs, 130);
        assert_eq!(stats.unique_tracks, 2);
        assert_eq!(stats.unique_artists, 2);
        assert!(stats.top_tracks.iter().all(|track| track.name != "Old Song"));
        assert!(stats.top_tracks.iter().any(|track| track.name == "Boundary Song"));
        assert!(stats.top_tracks.iter().any(|track| track.name == "New Song"));
    }

    #[test]
    fn test_top_tracks_aggregate_and_ranking_tie_breaks() {
        let records = vec![
            make_record("v1", "V1 Old", "Artist 1", 100, 30),
            make_record("v1", "V1 New", "Artist 1 Updated", 200, 50),
            make_record("v2", "Beta", "Artist 2", 300, 100),
            make_record("v3", "Alpha", "Artist 3", 400, 100),
            make_record("v4", "Gamma", "Artist 4", 500, 50),
        ];

        let stats = compute_listening_stats(&records, None, 10);
        assert_eq!(stats.top_tracks.len(), 4);
        assert_eq!(
            stats.top_tracks[0],
            RankedEntry {
                name: "V1 New".to_string(),
                detail: "Artist 1 Updated".to_string(),
                plays: 2,
                listened_secs: 80,
            }
        );
        assert_eq!(
            stats.top_tracks[1],
            RankedEntry {
                name: "Alpha".to_string(),
                detail: "Artist 3".to_string(),
                plays: 1,
                listened_secs: 100,
            }
        );
        assert_eq!(
            stats.top_tracks[2],
            RankedEntry {
                name: "Beta".to_string(),
                detail: "Artist 2".to_string(),
                plays: 1,
                listened_secs: 100,
            }
        );
        assert_eq!(
            stats.top_tracks[3],
            RankedEntry {
                name: "Gamma".to_string(),
                detail: "Artist 4".to_string(),
                plays: 1,
                listened_secs: 50,
            }
        );
    }

    #[test]
    fn test_top_artists_aggregation_and_empty_detail() {
        let records = vec![
            make_record("v1", "Song 1", "Rock Band", 100, 40),
            make_record("v2", "Song 2", "Rock Band", 200, 60),
            make_record("v3", "Song 3", "Pop Star", 300, 120),
        ];
        let stats = compute_listening_stats(&records, None, 5);
        assert_eq!(stats.top_artists.len(), 2);
        assert_eq!(
            stats.top_artists[0],
            RankedEntry {
                name: "Rock Band".to_string(),
                detail: String::new(),
                plays: 2,
                listened_secs: 100,
            }
        );
        assert_eq!(
            stats.top_artists[1],
            RankedEntry {
                name: "Pop Star".to_string(),
                detail: String::new(),
                plays: 1,
                listened_secs: 120,
            }
        );
    }

    #[test]
    fn test_limit_truncates_top_lists() {
        let records = vec![
            make_record("v1", "S1", "A1", 100, 10),
            make_record("v2", "S2", "A2", 200, 20),
            make_record("v3", "S3", "A3", 300, 30),
        ];
        let stats = compute_listening_stats(&records, None, 2);
        assert_eq!(stats.top_tracks.len(), 2);
        assert_eq!(stats.top_artists.len(), 2);
        assert_eq!(stats.unique_tracks, 3);
        assert_eq!(stats.unique_artists, 3);
    }

    #[test]
    fn test_stats_period_index_round_trip_and_defaults() {
        for period in [StatsPeriod::Last4Weeks, StatsPeriod::Last6Months, StatsPeriod::AllTime] {
            assert_eq!(StatsPeriod::from_index(period.to_index()), period);
        }
        assert_eq!(StatsPeriod::from_index(0), StatsPeriod::Last4Weeks);
        assert_eq!(StatsPeriod::from_index(1), StatsPeriod::Last6Months);
        assert_eq!(StatsPeriod::from_index(2), StatsPeriod::AllTime);
        assert_eq!(StatsPeriod::from_index(-1), StatsPeriod::AllTime);
        assert_eq!(StatsPeriod::from_index(3), StatsPeriod::AllTime);
        assert_eq!(StatsPeriod::from_index(100), StatsPeriod::AllTime);
        assert_eq!(StatsPeriod::default(), StatsPeriod::AllTime);
    }

    #[test]
    fn test_stats_period_window_days_and_since_unix() {
        assert_eq!(StatsPeriod::Last4Weeks.window_days(), Some(28));
        assert_eq!(StatsPeriod::Last6Months.window_days(), Some(182));
        assert_eq!(StatsPeriod::AllTime.window_days(), None);

        let now = 20_000_000;
        assert_eq!(StatsPeriod::Last4Weeks.since_unix(now), Some(now - 28 * SECS_PER_DAY));
        assert_eq!(StatsPeriod::Last6Months.since_unix(now), Some(now - 182 * SECS_PER_DAY));
        assert_eq!(StatsPeriod::AllTime.since_unix(now), None);

        // Saturating subtraction when current timestamp is smaller than window
        assert_eq!(StatsPeriod::Last4Weeks.since_unix(100), Some(0));
        assert_eq!(StatsPeriod::Last6Months.since_unix(0), Some(0));
    }
}
