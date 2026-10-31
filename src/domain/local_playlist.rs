//! Local playlist models and confidence thresholds for imported Spotify playlists.
//!
//! Invariant: this module performs no I/O, network calls, or clocks.

use serde::{Deserialize, Serialize};

use super::search::Track;

/// Prefix assigned to identifiers of imported playlists stored locally.
pub const LOCAL_PLAYLIST_PREFIX: &str = "local:";

/// Score threshold for high-confidence track matches.
pub const HIGH_CONFIDENCE_THRESHOLD: f32 = 0.85;

/// Score threshold for medium-confidence track matches.
pub const MEDIUM_CONFIDENCE_THRESHOLD: f32 = 0.60;

/// Metadata for one track as read from a Spotify export or the Web API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotifyTrack {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub isrc: Option<String>,
}

/// A Spotify playlist as read from an export or the Web API, before matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotifyPlaylist {
    pub id: String,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub tracks: Vec<SpotifyTrack>,
}

/// Confidence of a track match, derived from a normalized score in 0.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchConfidence {
    High,
    Medium,
    Low,
}

impl MatchConfidence {
    /// Derives confidence from a normalized score.
    ///
    /// Values `>= HIGH_CONFIDENCE_THRESHOLD` (including values `> 1.0`) yield [`MatchConfidence::High`].
    /// Values in `[MEDIUM_CONFIDENCE_THRESHOLD, HIGH_CONFIDENCE_THRESHOLD)` yield [`MatchConfidence::Medium`].
    /// Out-of-range negative values, values below `MEDIUM_CONFIDENCE_THRESHOLD`, and `NaN` yield [`MatchConfidence::Low`].
    pub fn from_score(score: f32) -> Self {
        if score.is_nan() || score < MEDIUM_CONFIDENCE_THRESHOLD {
            return Self::Low;
        }
        if score >= HIGH_CONFIDENCE_THRESHOLD {
            return Self::High;
        }
        Self::Medium
    }
}

/// One resolved local track inside an imported playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<u32>,
    pub confidence: MatchConfidence,
}

impl LocalTrack {
    /// Creates a new resolved local track hit with no album or duration.
    pub fn new(video_id: &str, title: &str, artist: &str, confidence: MatchConfidence) -> Self {
        Self {
            video_id: video_id.to_owned(),
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: None,
            duration_secs: None,
            confidence,
        }
    }

    /// Maps this resolved local track to a catalog [`Track`].
    pub fn to_track(&self, artwork: Option<String>) -> Track {
        Track {
            id: self.video_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_secs: self.duration_secs,
            artwork,
        }
    }
}

/// A playlist imported from Spotify and stored locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPlaylist {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub tracks: Vec<LocalTrack>,
}

impl LocalPlaylist {
    /// Creates a new empty local playlist with no author or artwork.
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            author: String::new(),
            artwork: None,
            tracks: Vec::new(),
        }
    }

    /// Sets the author display name.
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = author.into();
        self
    }

    /// Sets the playlist artwork URL.
    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        self.artwork = Some(artwork.into());
        self
    }

    /// Returns the number of resolved tracks in this playlist.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
}

/// Formats a local playlist identifier from a source identifier.
pub fn local_playlist_id(source_id: &str) -> String {
    format!("{LOCAL_PLAYLIST_PREFIX}{source_id}")
}

/// Returns `true` if the identifier is prefixed with [`LOCAL_PLAYLIST_PREFIX`].
pub fn is_local_playlist_id(id: &str) -> bool {
    id.starts_with(LOCAL_PLAYLIST_PREFIX) && id.len() > LOCAL_PLAYLIST_PREFIX.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_threshold_boundaries_table() {
        let cases = [
            (0.85, MatchConfidence::High),
            (0.8499, MatchConfidence::Medium),
            (0.60, MatchConfidence::Medium),
            (0.5999, MatchConfidence::Low),
            (f32::NAN, MatchConfidence::Low),
            (1.5, MatchConfidence::High),
            (-1.0, MatchConfidence::Low),
        ];

        for (score, expected) in cases {
            assert_eq!(MatchConfidence::from_score(score), expected, "failed for score: {score}");
        }
    }

    #[test]
    fn local_track_to_track_mapping() {
        let mut local =
            LocalTrack::new("video123", "Cool Track", "Cool Artist", MatchConfidence::High);
        local.album = Some("Cool Album".to_string());
        local.duration_secs = Some(215);

        let artwork = Some("https://example.com/art.png".to_string());
        let track = local.to_track(artwork.clone());

        assert_eq!(track.id, "video123");
        assert_eq!(track.title, "Cool Track");
        assert_eq!(track.artist, "Cool Artist");
        assert_eq!(track.album.as_deref(), Some("Cool Album"));
        assert_eq!(track.duration_secs, Some(215));
        assert_eq!(track.artwork, artwork);
    }

    #[test]
    fn serde_roundtrip_local_playlist() {
        let mut playlist = LocalPlaylist::new("local:test_id", "My Favorites")
            .with_author("Curator")
            .with_artwork("https://example.com/cover.jpg");
        let mut track1 = LocalTrack::new("vid_high", "Song 1", "Artist 1", MatchConfidence::High);
        track1.album = Some("Album 1".to_string());
        track1.duration_secs = Some(180);
        let track2 = LocalTrack::new("vid_med", "Song 2", "Artist 2", MatchConfidence::Medium);
        let track3 = LocalTrack::new("vid_low", "Song 3", "Artist 3", MatchConfidence::Low);
        playlist.tracks = vec![track1, track2, track3];

        let serialized = serde_json::to_string(&playlist).expect("serialization succeeds");
        let deserialized: LocalPlaylist =
            serde_json::from_str(&serialized).expect("deserialization succeeds");
        assert_eq!(playlist, deserialized);
    }

    #[test]
    fn spotify_models_serde_defaults() {
        let json = r#"{"id":"sp_123","title":"Pop","author":"Spotify"}"#;
        let playlist: SpotifyPlaylist = serde_json::from_str(json).expect("parses with defaults");
        assert_eq!(playlist.artwork, None);
        assert!(playlist.tracks.is_empty());

        let track_json = r#"{"title":"Song","artist":"Singer"}"#;
        let track: SpotifyTrack = serde_json::from_str(track_json).expect("parses with defaults");
        assert_eq!(track.album, None);
        assert_eq!(track.duration_ms, 0);
        assert_eq!(track.isrc, None);
    }

    #[test]
    fn local_playlist_id_symmetry() {
        let cases = ["37i9dQZF1DXcBWIGoYBM5M", "custom-playlist", "123_456"];
        for source in cases {
            let generated = local_playlist_id(source);
            assert!(is_local_playlist_id(&generated));
            assert!(!is_local_playlist_id(source));
        }
        assert!(!is_local_playlist_id(""));
        assert!(!is_local_playlist_id(LOCAL_PLAYLIST_PREFIX));
        assert!(!is_local_playlist_id("spotify:playlist:123"));
    }
}
