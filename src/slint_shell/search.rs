//! Slint search page state and view-model bridge.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, and zero async runtimes.
//!
//! Async boundary note: In future tranches, search queries and home shelves will be
//! fetched asynchronously from the Catalog port adapter. This sync bridge provides
//! the pure in-memory boundary holding query text, domain shelves, and flattened
//! display models.

use crate::domain::{SearchFilter, SearchResult, Shelf};

/// Flattened display and navigation row for all catalog item kinds in the search view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRow {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub album: String,
    pub duration: String,
    pub artwork_url: Option<String>,
}

impl SearchRow {
    /// Sets optional artwork thumbnail URL, consuming self.
    pub fn with_artwork(mut self, artwork_url: impl Into<String>) -> Self {
        self.artwork_url = Some(artwork_url.into());
        self
    }
}

impl From<&SearchResult> for SearchRow {
    fn from(item: &SearchResult) -> Self {
        match item {
            SearchResult::Track(track) => SearchRow {
                id: track.id.clone(),
                kind: "track".to_string(),
                title: track.title.clone(),
                subtitle: track.artist.clone(),
                album: track.album.clone().unwrap_or_default(),
                duration: format_duration(track.duration_secs),
                artwork_url: track.artwork.clone(),
            },
            SearchResult::Album(album) => SearchRow {
                id: album.id.clone(),
                kind: "album".to_string(),
                title: album.title.clone(),
                subtitle: album.artist.clone(),
                album: String::new(),
                duration: String::new(),
                artwork_url: album.artwork.clone(),
            },
            SearchResult::Artist(artist) => SearchRow {
                id: artist.id.clone(),
                kind: "artist".to_string(),
                title: artist.name.clone(),
                subtitle: String::new(),
                album: String::new(),
                duration: String::new(),
                artwork_url: artist.artwork.clone(),
            },
            SearchResult::Playlist(playlist) => SearchRow {
                id: playlist.id.clone(),
                kind: "playlist".to_string(),
                title: playlist.title.clone(),
                subtitle: if playlist.track_count == 0 {
                    String::new()
                } else {
                    format!("{} tracks", playlist.track_count)
                },
                album: String::new(),
                duration: String::new(),
                artwork_url: playlist.artwork.clone(),
            },
        }
    }
}

impl From<SearchResult> for SearchRow {
    fn from(item: SearchResult) -> Self {
        SearchRow::from(&item)
    }
}

/// Formats track duration seconds into standard mm:ss display text.
pub fn format_duration(duration_secs: Option<u32>) -> String {
    let Some(secs) = duration_secs else {
        return String::new();
    };
    let minutes = secs / 60;
    let seconds = secs % 60;
    format!("{minutes}:{seconds:02}")
}

/// Returns true when a search result matches the specified kind filter.
pub fn matches_filter(item: &SearchResult, filter: SearchFilter) -> bool {
    filter == SearchFilter::All || item.kind() == filter
}

/// Helper extracting an iterator over all search result references across shelves matching filter.
pub fn iter_shelf_items(
    shelves: &[Shelf],
    filter: SearchFilter,
) -> impl Iterator<Item = &SearchResult> {
    shelves
        .iter()
        .flat_map(|shelf| shelf.items.iter())
        .filter(move |item| matches_filter(item, filter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, Playlist, Track};

    #[test]
    fn test_format_duration_variants() {
        assert_eq!(format_duration(None), "");
        assert_eq!(format_duration(Some(0)), "0:00");
        assert_eq!(format_duration(Some(9)), "0:09");
        assert_eq!(format_duration(Some(60)), "1:00");
        assert_eq!(format_duration(Some(65)), "1:05");
        assert_eq!(format_duration(Some(599)), "9:59");
        assert_eq!(format_duration(Some(3600)), "60:00");
    }

    #[test]
    fn test_search_row_kind_mappings() {
        let track = Track::new("t1", "Track One", "Artist One")
            .with_album("Album One")
            .with_duration(200)
            .with_artwork("https://artwork.local/t1");
        let row_track = SearchRow::from(&SearchResult::Track(track));
        assert_eq!(row_track.id, "t1");
        assert_eq!(row_track.kind, "track");
        assert_eq!(row_track.title, "Track One");
        assert_eq!(row_track.subtitle, "Artist One");
        assert_eq!(row_track.album, "Album One");
        assert_eq!(row_track.duration, "3:20");
        assert_eq!(row_track.artwork_url.as_deref(), Some("https://artwork.local/t1"));

        let album = Album::new(
            "a1",
            "Album Title",
            "Album Artist",
            Some("https://artwork.local/a1".into()),
        );
        let row_album = SearchRow::from(&SearchResult::Album(album));
        assert_eq!(row_album.id, "a1");
        assert_eq!(row_album.kind, "album");
        assert_eq!(row_album.title, "Album Title");
        assert_eq!(row_album.subtitle, "Album Artist");
        assert_eq!(row_album.album, "");
        assert_eq!(row_album.duration, "");
        assert_eq!(row_album.artwork_url.as_deref(), Some("https://artwork.local/a1"));

        let artist = Artist::new("ar1", "Artist Name", Some("https://artwork.local/ar1".into()));
        let row_artist = SearchRow::from(&SearchResult::Artist(artist));
        assert_eq!(row_artist.id, "ar1");
        assert_eq!(row_artist.kind, "artist");
        assert_eq!(row_artist.title, "Artist Name");
        assert_eq!(row_artist.subtitle, "");
        assert_eq!(row_artist.album, "");
        assert_eq!(row_artist.duration, "");
        assert_eq!(row_artist.artwork_url.as_deref(), Some("https://artwork.local/ar1"));

        let playlist =
            Playlist::new("p1", "My Playlist", 15, Some("https://artwork.local/p1".into()));
        let row_playlist = SearchRow::from(&SearchResult::Playlist(playlist));
        assert_eq!(row_playlist.id, "p1");
        assert_eq!(row_playlist.kind, "playlist");
        assert_eq!(row_playlist.title, "My Playlist");
        assert_eq!(row_playlist.subtitle, "15 tracks");
        assert_eq!(row_playlist.album, "");
        assert_eq!(row_playlist.duration, "");
        assert_eq!(row_playlist.artwork_url.as_deref(), Some("https://artwork.local/p1"));
    }

    #[test]
    fn test_search_row_empty_subtitle_handling() {
        let artist = Artist::new("ar1", "Solo", None);
        let row = SearchRow::from(&SearchResult::Artist(artist));
        assert_eq!(row.subtitle, "");

        let track = Track::new("t1", "No Artist Song", "");
        let track_row = SearchRow::from(&SearchResult::Track(track));
        assert_eq!(track_row.subtitle, "");

        let playlist = Playlist::new("p0", "Empty Playlist", 0, None);
        let pl_row = SearchRow::from(&SearchResult::Playlist(playlist));
        assert_eq!(pl_row.subtitle, "");
    }
}
