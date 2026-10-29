//! Slint library page state and view-model bridge.
//! Hexagonal invariant: pure in-memory with zero I/O, network, audio, or async.

use crate::domain::{Album, Artist, Playlist};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryItemRow {
    pub title: String,
    pub subtitle: String,
    pub artwork_url: Option<String>,
}

impl LibraryItemRow {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self { title: title.into(), subtitle: subtitle.into(), artwork_url: None }
    }

    pub fn with_artwork(mut self, artwork_url: impl Into<String>) -> Self {
        self.artwork_url = Some(artwork_url.into());
        self
    }
}

pub fn playlist_row(playlist: &Playlist) -> LibraryItemRow {
    LibraryItemRow {
        title: playlist.title.clone(),
        subtitle: format!("{} tracks", playlist.track_count),
        artwork_url: playlist.artwork.clone(),
    }
}

pub fn album_row(album: &Album) -> LibraryItemRow {
    LibraryItemRow {
        title: album.title.clone(),
        subtitle: album.artist.clone(),
        artwork_url: album.artwork.clone(),
    }
}

pub fn artist_row(artist: &Artist) -> LibraryItemRow {
    LibraryItemRow {
        title: artist.name.clone(),
        subtitle: "Artist".to_string(),
        artwork_url: artist.artwork.clone(),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryState {
    pub playlists: Vec<Playlist>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl LibraryState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_data(playlists: Vec<Playlist>, albums: Vec<Album>, artists: Vec<Artist>) -> Self {
        Self { playlists, albums, artists }
    }

    pub fn set_playlists(&mut self, playlists: Vec<Playlist>) {
        self.playlists = playlists;
    }

    pub fn set_albums(&mut self, albums: Vec<Album>) {
        self.albums = albums;
    }

    pub fn set_artists(&mut self, artists: Vec<Artist>) {
        self.artists = artists;
    }

    pub fn clear(&mut self) {
        self.playlists.clear();
        self.albums.clear();
        self.artists.clear();
    }

    pub fn selected_playlist(&self, index: usize) -> Option<&Playlist> {
        self.playlists.get(index)
    }

    pub fn selected_album(&self, index: usize) -> Option<&Album> {
        self.albums.get(index)
    }

    pub fn selected_artist(&self, index: usize) -> Option<&Artist> {
        self.artists.get(index)
    }

    pub fn playlist_rows(&self) -> Vec<LibraryItemRow> {
        self.playlists.iter().map(playlist_row).collect()
    }

    pub fn album_rows(&self) -> Vec<LibraryItemRow> {
        self.albums.iter().map(album_row).collect()
    }

    pub fn artist_rows(&self) -> Vec<LibraryItemRow> {
        self.artists.iter().map(artist_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_state_lifecycle_and_selections() {
        let mut state = LibraryState::new();
        assert!(state.playlists.is_empty() && state.albums.is_empty() && state.artists.is_empty());
        assert_eq!(state.selected_playlist(0), None);
        assert_eq!(state.selected_album(0), None);
        assert_eq!(state.selected_artist(0), None);

        state.set_playlists(vec![Playlist::new("p1", "Favs", 5, None)]);
        state.set_albums(vec![Album::new("a1", "Discovery", "Daft Punk", None)]);
        state.set_artists(vec![Artist::new("ar1", "Daft Punk", None)]);
        assert_eq!(state.selected_playlist(0).map(|p| p.id.as_str()), Some("p1"));
        assert_eq!(state.selected_playlist(1), None);
        assert_eq!(state.selected_album(0).map(|a| a.id.as_str()), Some("a1"));
        assert_eq!(state.selected_album(1), None);
        assert_eq!(state.selected_artist(0).map(|a| a.id.as_str()), Some("ar1"));
        assert_eq!(state.selected_artist(1), None);

        state.clear();
        assert!(state.playlists.is_empty() && state.albums.is_empty() && state.artists.is_empty());
    }

    #[test]
    fn test_library_display_rows_match_domain_data() {
        let p = Playlist::new("p1", "Favorites", 25, None);
        let a = Album::new("a1", "Discovery", "Daft Punk", None);
        let ar = Artist::new("ar1", "Daft Punk", None);
        let state = LibraryState::with_data(vec![p.clone()], vec![a.clone()], vec![ar.clone()]);

        assert_eq!(playlist_row(&p), LibraryItemRow::new("Favorites", "25 tracks"));
        assert_eq!(album_row(&a), LibraryItemRow::new("Discovery", "Daft Punk"));
        assert_eq!(artist_row(&ar), LibraryItemRow::new("Daft Punk", "Artist"));
        assert_eq!(state.playlist_rows(), vec![playlist_row(&p)]);
        assert_eq!(state.album_rows(), vec![album_row(&a)]);
        assert_eq!(state.artist_rows(), vec![artist_row(&ar)]);
    }

    #[test]
    fn test_library_display_rows_with_artwork() {
        let p = Playlist::new("p1", "Favorites", 25, Some("https://example.com/p1.jpg".into()));
        let a =
            Album::new("a1", "Discovery", "Daft Punk", Some("https://example.com/a1.jpg".into()));
        let ar = Artist::new("ar1", "Daft Punk", Some("https://example.com/ar1.jpg".into()));

        assert_eq!(
            playlist_row(&p),
            LibraryItemRow::new("Favorites", "25 tracks")
                .with_artwork("https://example.com/p1.jpg")
        );
        assert_eq!(
            album_row(&a),
            LibraryItemRow::new("Discovery", "Daft Punk")
                .with_artwork("https://example.com/a1.jpg")
        );
        assert_eq!(
            artist_row(&ar),
            LibraryItemRow::new("Daft Punk", "Artist").with_artwork("https://example.com/ar1.jpg")
        );
    }
}
