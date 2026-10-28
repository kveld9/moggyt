//! Slint UI shell foundation: page navigation state and window bindings.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, and zero async runtimes.
//!
//! Tranche 2 wiring points (domain types to bind in subsequent tranches):
//! - SlintPage::Search:
//!     - crate::domain::Shelf (catalog home shelves and carousels)
//!     - crate::domain::Track (search query result items)
//!     - crate::domain::SearchFilter (catalog filter categories)
//! - SlintPage::Queue:
//!     - crate::domain::PlaybackQueue (cursor, track list, queue reordering)
//!     - crate::domain::RepeatMode (repeat state: Off, All, One)
//! - SlintPage::NowPlaying:
//!     - crate::domain::Track (active track metadata, duration, cover art URL)
//!     - crate::domain::SkipRanges (auto-skip sponsor and silence segments)
//! - SlintPage::Library:
//!     - crate::domain::Playlist (saved user playlists)
//!     - crate::domain::Album (saved albums)
//!     - crate::domain::Artist (followed artists)
//! - SlintPage::Detail:
//!     - crate::domain::Detail (container model for album, artist, or playlist)
//!     - crate::domain::DetailKind (Album, Artist, Playlist discriminator)

slint::include_modules!();

pub use MoggytWindow as AppWindow;

pub mod detail;
pub mod library;
pub mod platform;
pub mod playback;
pub mod search;

pub use detail::{DetailState, detail_kind_label, detail_track_rows};
pub use library::{LibraryItemRow, LibraryState, album_row, artist_row, playlist_row};
pub use platform::{
    CoverArtData, LyricDisplayLine, LyricsState, PlatformState, PresenceSnapshot, Transport,
    TransportCommand, TransportStub, dispatch_to_ui, lyrics_display_lines, rgba_to_image,
};
pub use playback::{PlaybackState, format_position, format_track_duration, queue_track_rows};
pub use search::{SearchState, TrackDisplayRow, canned_shelves, format_duration, shelf_tracks};

/// Navigation pages of the Slint UI shell, matching domain Page order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SlintPage {
    /// Search catalog and browse home shelves.
    #[default]
    Search,
    /// Inspect and manage playback queue.
    Queue,
    /// Active playback and lyrics display.
    NowPlaying,
    /// User library containing playlists, albums, and tracks.
    Library,
    /// Detailed album, artist, or playlist view.
    Detail,
}

impl SlintPage {
    /// Resolves page from index, falling back to Search for out-of-bounds values.
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Search,
            1 => Self::Queue,
            2 => Self::NowPlaying,
            3 => Self::Library,
            4 => Self::Detail,
            _ => Self::Search,
        }
    }

    /// Converts page variant to its 0-based integer index.
    pub fn to_index(self) -> i32 {
        match self {
            Self::Search => 0,
            Self::Queue => 1,
            Self::NowPlaying => 2,
            Self::Library => 3,
            Self::Detail => 4,
        }
    }

    /// Returns human-readable page title for display in the shell header.
    pub fn title(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::Queue => "Queue",
            Self::NowPlaying => "Now Playing",
            Self::Library => "Library",
            Self::Detail => "Detail",
        }
    }
}

/// Pure in-memory UI shell state for navigation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlintShellState {
    /// Currently active navigation page.
    pub page: SlintPage,
}

impl SlintShellState {
    /// Creates a new default shell state with the Search page active.
    pub fn new() -> Self {
        Self { page: SlintPage::Search }
    }

    /// Selects a new active page.
    pub fn select_page(&mut self, page: SlintPage) {
        self.page = page;
    }

    /// Selects a new active page by index, falling back to Search if out of bounds.
    pub fn select_page_from_index(&mut self, index: i32) {
        self.page = SlintPage::from_index(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slint_page_from_index_transitions() {
        assert_eq!(SlintPage::from_index(0), SlintPage::Search);
        assert_eq!(SlintPage::from_index(1), SlintPage::Queue);
        assert_eq!(SlintPage::from_index(2), SlintPage::NowPlaying);
        assert_eq!(SlintPage::from_index(3), SlintPage::Library);
        assert_eq!(SlintPage::from_index(4), SlintPage::Detail);
    }

    #[test]
    fn test_slint_page_from_index_out_of_bounds_fallback() {
        assert_eq!(SlintPage::from_index(-1), SlintPage::Search);
        assert_eq!(SlintPage::from_index(5), SlintPage::Search);
        assert_eq!(SlintPage::from_index(100), SlintPage::Search);
        assert_eq!(SlintPage::from_index(i32::MIN), SlintPage::Search);
        assert_eq!(SlintPage::from_index(i32::MAX), SlintPage::Search);
    }

    #[test]
    fn test_slint_page_to_index_mapping() {
        assert_eq!(SlintPage::Search.to_index(), 0);
        assert_eq!(SlintPage::Queue.to_index(), 1);
        assert_eq!(SlintPage::NowPlaying.to_index(), 2);
        assert_eq!(SlintPage::Library.to_index(), 3);
        assert_eq!(SlintPage::Detail.to_index(), 4);
    }

    #[test]
    fn test_slint_page_title_mapping() {
        assert_eq!(SlintPage::Search.title(), "Search");
        assert_eq!(SlintPage::Queue.title(), "Queue");
        assert_eq!(SlintPage::NowPlaying.title(), "Now Playing");
        assert_eq!(SlintPage::Library.title(), "Library");
        assert_eq!(SlintPage::Detail.title(), "Detail");
    }

    #[test]
    fn test_slint_shell_state_default() {
        let state = SlintShellState::default();
        assert_eq!(state.page, SlintPage::Search);

        let new_state = SlintShellState::new();
        assert_eq!(new_state.page, SlintPage::Search);
    }

    #[test]
    fn test_slint_shell_state_select_page() {
        let mut state = SlintShellState::new();
        assert_eq!(state.page, SlintPage::Search);

        state.select_page(SlintPage::Queue);
        assert_eq!(state.page, SlintPage::Queue);

        state.select_page(SlintPage::NowPlaying);
        assert_eq!(state.page, SlintPage::NowPlaying);

        state.select_page(SlintPage::Library);
        assert_eq!(state.page, SlintPage::Library);

        state.select_page(SlintPage::Detail);
        assert_eq!(state.page, SlintPage::Detail);

        state.select_page(SlintPage::Search);
        assert_eq!(state.page, SlintPage::Search);
    }

    #[test]
    fn test_slint_shell_state_select_page_from_index() {
        let mut state = SlintShellState::new();

        state.select_page_from_index(1);
        assert_eq!(state.page, SlintPage::Queue);

        state.select_page_from_index(3);
        assert_eq!(state.page, SlintPage::Library);

        state.select_page_from_index(999);
        assert_eq!(state.page, SlintPage::Search);

        state.select_page_from_index(-42);
        assert_eq!(state.page, SlintPage::Search);
    }
}
