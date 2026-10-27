//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod library;
mod metadata;
mod playlist_import;
mod queue;
mod search;
mod segments;
mod sleep_timer;
mod theme;

pub use library::{
    Album, Artist, Detail, DetailKind, Playlist, SearchPage, SearchResult, Shelf, apply_filter,
};
pub use metadata::MetadataOverride;
pub use playlist_import::extract_playlist_id;
pub use queue::{PlaybackQueue, RepeatMode};
pub use search::{
    LyricsSnippet, LyricsSnippetError, MAX_LYRICS_SNIPPET_LEN, SearchFilter, SearchQuery, Track,
};
pub use segments::{InvalidSegment, SkipRanges, SkipSegment};
pub use sleep_timer::{SleepTimer, TimerPreset};
pub use theme::{DesignTokens, Palette, ThemeId};
