//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod library;
mod local_playlist;
mod matching;
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
pub use matching::{
    DURATION_EXACT_BONUS, DURATION_EXACT_SECS, DURATION_SUSPECT_PENALTY, DURATION_SUSPECT_SECS,
    DURATION_TOLERANCE_SECS, DURATION_VIDEO_PENALTY, DURATION_WRONG_EDIT_PENALTY,
    DURATION_WRONG_EDIT_SECS, MatchCandidate, MatchScore, MusicVideoType, PRISM_ATV_BONUS,
    PRISM_KEYWORD_PENALTY, PRISM_OMV_PENALTY, PRISM_TOPIC_BONUS, PRISM_UGC_PENALTY,
    WEIGHT_ARTIST_PRIMARY, WEIGHT_ARTIST_SECONDARY, WEIGHT_TITLE, artist_score, clean_track_title,
    confidence_for, duration_score, levenshtein, normalize_for_match, prism_video_penalty,
    score_candidate, similarity, split_featured, title_similarity, token_set,
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
