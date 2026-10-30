//! Navigation pages of the Slint UI shell.
//!
//! Hexagonal invariant: pure in-memory enum representing UI pages with zero I/O.

use crate::i18n::{Locale, tr};

/// Navigation pages of the Slint UI shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SlintPage {
    /// Search catalog and browse home shelves.
    #[default]
    Search,
    /// Active playback and lyrics display.
    NowPlaying,
    /// User library containing playlists, albums, and tracks.
    Library,
    /// Detailed album, artist, or playlist view.
    Detail,
    /// Application settings for theme, playback, session, and about.
    Settings,
    /// Explore section with charts, genres, moods, and new releases.
    Explore,
}

impl SlintPage {
    /// Resolves page from index, falling back to Search for out-of-bounds values.
    pub fn from_index(index: i32) -> Self {
        match index {
            0 => Self::Search,
            1 => Self::NowPlaying,
            2 => Self::Library,
            3 => Self::Detail,
            4 => Self::Settings,
            5 => Self::Explore,
            _ => Self::Search,
        }
    }

    /// Converts page variant to its 0-based integer index.
    pub fn to_index(self) -> i32 {
        match self {
            Self::Search => 0,
            Self::NowPlaying => 1,
            Self::Library => 2,
            Self::Detail => 3,
            Self::Settings => 4,
            Self::Explore => 5,
        }
    }

    /// Returns human-readable page title for display in the shell header.
    pub fn title(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::NowPlaying => "Now Playing",
            Self::Library => "Library",
            Self::Detail => "Detail",
            Self::Settings => "Settings",
            Self::Explore => "Explore",
        }
    }

    /// Returns localized page title for display in the shell header.
    pub fn title_with_locale(self, locale: Locale) -> &'static str {
        tr(locale, self.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_page_navigation() {
        let mut state = crate::slint_shell::test_support::test_wired();
        assert_eq!(state.page, SlintPage::Search);
        state.select_page(SlintPage::Settings);
        assert_eq!(state.page, SlintPage::Settings);
        assert_eq!(state.page.to_index(), 4);
        assert_eq!(state.page.title(), "Settings");
        assert_eq!(state.page.title_with_locale(Locale::Spanish), "Ajustes");
        assert_eq!(SlintPage::from_index(4), SlintPage::Settings);
    }

    #[test]
    fn test_explore_page_indexing_and_titles() {
        assert_eq!(SlintPage::from_index(5), SlintPage::Explore);
        assert_eq!(SlintPage::Explore.to_index(), 5);
        assert_eq!(SlintPage::Explore.title(), "Explore");
    }

    #[test]
    fn test_slint_page_all_variants_round_trip() {
        let pages = [
            (0, SlintPage::Search, "Search"),
            (1, SlintPage::NowPlaying, "Now Playing"),
            (2, SlintPage::Library, "Library"),
            (3, SlintPage::Detail, "Detail"),
            (4, SlintPage::Settings, "Settings"),
            (5, SlintPage::Explore, "Explore"),
        ];
        for (idx, page, title) in pages {
            assert_eq!(SlintPage::from_index(idx), page);
            assert_eq!(page.to_index(), idx);
            assert_eq!(page.title(), title);
        }
        assert_eq!(SlintPage::from_index(999), SlintPage::Search);
    }
}
