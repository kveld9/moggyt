//! Pure domain: playback queue and search models.
//!
//! Invariant: this module performs no I/O. Network, filesystem, clocks, and
//! randomness stay behind ports in `crate::infra` or parameters.

mod library;
mod queue;
mod search;
mod theme;

pub use library::{Album, Artist, Playlist, SearchPage, SearchResult, Shelf, apply_filter};
pub use queue::{PlaybackQueue, RepeatMode};
pub use search::{SearchFilter, SearchQuery, Track};
pub use theme::{Palette, ThemeId};
