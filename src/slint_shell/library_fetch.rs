//! Library sections fetched from the catalog port in one refresh.

use crate::domain::{Album, Artist, Playlist, Track};
use crate::infra::CatalogError;

/// Per-section catalog results for one library refresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFetch {
    pub playlists: Result<Vec<Playlist>, CatalogError>,
    pub songs: Result<Vec<Track>, CatalogError>,
    pub albums: Result<Vec<Album>, CatalogError>,
    pub artists: Result<Vec<Artist>, CatalogError>,
}

/// Successful library contents with every section resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibrarySections {
    pub playlists: Vec<Playlist>,
    pub songs: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl LibraryFetch {
    /// Resolves every section, returning the first failure when one exists.
    pub(crate) fn into_sections(self) -> Result<LibrarySections, CatalogError> {
        Ok(LibrarySections {
            playlists: self.playlists?,
            songs: self.songs?,
            albums: self.albums?,
            artists: self.artists?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(
        playlists: Result<Vec<Playlist>, CatalogError>,
        songs: Result<Vec<Track>, CatalogError>,
        albums: Result<Vec<Album>, CatalogError>,
        artists: Result<Vec<Artist>, CatalogError>,
    ) -> LibraryFetch {
        LibraryFetch { playlists, songs, albums, artists }
    }

    #[test]
    fn into_sections_collects_every_section() {
        let sections = fetch(
            Ok(vec![Playlist::new("p1", "Mix", 2, None)]),
            Ok(vec![Track::new("t1", "Song", "Artist")]),
            Ok(Vec::new()),
            Ok(vec![Artist::new("a1", "Artist", None)]),
        )
        .into_sections()
        .expect("sections");
        assert_eq!(sections.playlists.len(), 1);
        assert!(sections.albums.is_empty());
        assert_eq!(sections.artists.len(), 1);
    }

    #[test]
    fn into_sections_reports_the_first_failure() {
        let error = fetch(
            Ok(Vec::new()),
            Ok(Vec::new()),
            Err(CatalogError::Unauthenticated),
            Err(CatalogError::Transport),
        )
        .into_sections()
        .expect_err("failure");
        assert_eq!(error, CatalogError::Unauthenticated);
    }
}
