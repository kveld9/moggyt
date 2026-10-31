//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod library;
mod local_playlist;
mod playlist_import;
mod queue;
mod search;
mod segments;
mod sleep_timer;
mod theme;

pub use library::{Album, Artist, Detail, DetailKind, Playlist, SearchResult, Shelf};
pub use local_playlist::{
    HIGH_CONFIDENCE_THRESHOLD, LOCAL_PLAYLIST_PREFIX, LocalPlaylist, LocalTrack,
    MEDIUM_CONFIDENCE_THRESHOLD, MatchConfidence, SpotifyPlaylist, SpotifyTrack,
    is_local_playlist_id, local_playlist_id,
};
pub use playlist_import::{SpotifyRef, extract_playlist_id, parse_spotify_ref};
pub use queue::{PlaybackQueue, RepeatMode};
pub use search::{
    LyricsSnippet, LyricsSnippetError, MAX_LYRICS_SNIPPET_LEN, RECENT_SEARCH_LIMIT, RecentSearches,
    SearchFilter, SearchQuery, Track,
};
pub use segments::{InvalidSegment, SkipRanges, SkipSegment};
pub use sleep_timer::{SleepTimer, TimerPreset};
pub use theme::{Palette, PlayerBarMode, ThemeId};
