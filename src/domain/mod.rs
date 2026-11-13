//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod artist_match;
mod library;
mod listening_stats;
mod local_playlist;
mod matching;
mod play_log;
mod playback_phase;
mod playlist_import;
mod queue;
mod search;
mod segments;
mod shuffle;
mod sleep_timer;
mod theme;
mod top_result;
mod version_override;
mod videos;

pub use artist_match::{
    TOPIC_CHANNEL_SUFFIX, artist_score, split_source_artists, strip_topic_suffix,
};
pub use library::{
    Album, Artist, ArtistPage, Detail, DetailCard, DetailCardKind, DetailKind, DetailSection,
    Playlist, SearchResult, Shelf,
};
pub use listening_stats::{
    ListeningStats, RankedEntry, SECS_PER_DAY, STATS_TOP_LIMIT, StatsPeriod,
    compute_listening_stats,
};
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
    WEIGHT_ARTIST_PRIMARY, WEIGHT_ARTIST_SECONDARY, WEIGHT_TITLE, clean_track_title,
    confidence_for, duration_score, levenshtein, normalize_for_match, prism_video_penalty,
    score_candidate, similarity, split_featured, title_similarity, token_set,
};
pub use play_log::{MIN_RECORDED_PLAY_SECS, PlayRecord, listened_secs, record_for_play};
pub use playback_phase::{END_TOLERANCE_SECS, PlaybackEvent, PlaybackFacts, PlaybackPhase};
pub use playlist_import::{SpotifyRef, extract_playlist_id, parse_spotify_ref};
pub use queue::{PlaybackQueue, RepeatMode};
pub use search::{
    LyricsSnippet, LyricsSnippetError, MAX_LYRICS_SNIPPET_LEN, RECENT_SEARCH_LIMIT, RecentSearches,
    SearchFilter, SearchQuery, Track, retain_song_results, retain_songs,
};
pub use segments::{InvalidSegment, SkipRanges, SkipSegment};
pub use shuffle::{RECENT_PLAY_WINDOW, smart_order};
pub use sleep_timer::{SLEEP_FADE_WINDOW_SECS, SleepTimer, TimerPreset, faded_volume};
pub use theme::{
    CustomPaletteError, Palette, PlayerBarMode, ThemeId, format_hex_rgb, parse_hex_rgb,
};
pub use top_result::{MIN_TOP_RESULT_SCORE, best_match, entity_name, score_top_result};
pub use version_override::{MAX_VERSION_OVERRIDES, VersionOverrideError, VersionOverrides};
pub use videos::{apply_detail_policy, apply_result_policy, apply_shelf_policy, songs_first};
