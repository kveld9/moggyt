//! Slint search page state and view-model bridge.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, and zero async runtimes.
//!
//! Async boundary note: In future tranches, search queries and home shelves will be
//! fetched asynchronously from the Catalog port adapter. This sync bridge provides
//! the pure in-memory boundary holding query text, domain shelves, and flattened
//! display models.

use crate::domain::{SearchFilter, SearchResult, Shelf, Track};

/// Parts required to construct a [`SearchRow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRowParts {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub album: String,
    pub duration: String,
}

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
    /// Creates a search row from bundled parts with no artwork.
    pub fn from_parts(parts: SearchRowParts) -> Self {
        Self {
            id: parts.id,
            kind: parts.kind,
            title: parts.title,
            subtitle: parts.subtitle,
            album: parts.album,
            duration: parts.duration,
            artwork_url: None,
        }
    }

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

/// Flattened display row for track items in the Slint search view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackDisplayRow {
    pub title: String,
    pub artist: String,
    pub duration: String,
}

impl TrackDisplayRow {
    /// Creates a display row from explicit title, artist, and duration text.
    pub fn new(
        title: impl Into<String>,
        artist: impl Into<String>,
        duration: impl Into<String>,
    ) -> Self {
        Self { title: title.into(), artist: artist.into(), duration: duration.into() }
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

/// Flattens domain shelves into a list of search rows filtered by item kind.
pub fn shelf_search_rows(shelves: &[Shelf], filter: SearchFilter) -> Vec<SearchRow> {
    iter_shelf_items(shelves, filter).map(SearchRow::from).collect()
}

/// Helper extracting an iterator over all track references across shelves.
fn iter_shelf_tracks(shelves: &[Shelf]) -> impl Iterator<Item = &Track> {
    shelves.iter().flat_map(|shelf| {
        shelf.items.iter().filter_map(|item| match item {
            SearchResult::Track(track) => Some(track),
            _ => None,
        })
    })
}

/// Flattens domain shelves into a list of display rows for the Slint UI.
pub fn shelf_tracks(shelves: &[Shelf]) -> Vec<TrackDisplayRow> {
    iter_shelf_tracks(shelves)
        .map(|track| TrackDisplayRow {
            title: track.title.clone(),
            artist: track.artist.clone(),
            duration: format_duration(track.duration_secs),
        })
        .collect()
}

/// In-memory state for the Slint search view.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SearchState {
    /// Active query text.
    pub query: String,
    /// Loaded catalog shelves.
    pub shelves: Vec<Shelf>,
    /// Active category filter.
    pub filter: SearchFilter,
}

impl SearchState {
    /// Creates an empty search state.
    pub fn new() -> Self {
        Self { query: String::new(), shelves: Vec::new(), filter: SearchFilter::All }
    }

    /// Creates a search state populated with canned shelves for UI preview.
    pub fn with_canned_data() -> Self {
        Self { query: String::new(), shelves: canned_shelves(), filter: SearchFilter::All }
    }

    /// Updates the active category filter.
    pub fn set_filter(&mut self, filter: SearchFilter) {
        self.filter = filter;
    }

    /// Returns the active category filter.
    pub fn filter(&self) -> SearchFilter {
        self.filter
    }

    /// Updates the search query text.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    /// Updates the loaded catalog shelves.
    pub fn set_shelves(&mut self, shelves: Vec<Shelf>) {
        self.shelves = shelves;
    }

    /// Resolves search result item by 0-based flattened index under active filter.
    pub fn selected_item(&self, index: usize) -> Option<&SearchResult> {
        iter_shelf_items(&self.shelves, self.filter).nth(index)
    }

    /// Returns flattened search rows corresponding to items matching active filter.
    pub fn search_rows(&self) -> Vec<SearchRow> {
        shelf_search_rows(&self.shelves, self.filter)
    }

    /// Resolves track by 0-based flattened index, returning None if out of bounds.
    pub fn selected_track(&self, index: usize) -> Option<&Track> {
        iter_shelf_tracks(&self.shelves).nth(index)
    }

    /// Returns flattened display rows corresponding to all tracks across loaded shelves.
    pub fn shelf_tracks(&self) -> Vec<TrackDisplayRow> {
        shelf_tracks(&self.shelves)
    }
}

/// Deterministic canned shelves for development preview and sync testing.
pub fn canned_shelves() -> Vec<Shelf> {
    vec![
        Shelf {
            title: "Featured Tracks".to_owned(),
            items: vec![
                SearchResult::Track(
                    Track::new("canned-1", "Midnight City", "M83").with_duration(243),
                ),
                SearchResult::Track(
                    Track::new("canned-2", "Starboy", "The Weeknd").with_duration(230),
                ),
            ],
        },
        Shelf {
            title: "Recommended".to_owned(),
            items: vec![SearchResult::Track(
                Track::new("canned-3", "Get Lucky", "Daft Punk").with_duration(248),
            )],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, Playlist};
    use crate::infra::{Catalog, CatalogStub};

    #[test]
    fn test_search_state_new_and_default() {
        let state = SearchState::new();
        assert_eq!(state.query, "");
        assert!(state.shelves.is_empty());
        assert_eq!(state, SearchState::default());
    }

    #[test]
    fn test_search_state_set_query_transitions() {
        let mut state = SearchState::new();
        assert_eq!(state.query, "");

        state.set_query("daft punk");
        assert_eq!(state.query, "daft punk");

        state.set_query("radiohead");
        assert_eq!(state.query, "radiohead");

        state.set_query("");
        assert_eq!(state.query, "");
    }

    #[test]
    fn test_search_state_set_shelves_transitions() {
        let mut state = SearchState::new();
        assert!(state.shelves.is_empty());

        let canned = canned_shelves();
        state.set_shelves(canned.clone());
        assert_eq!(state.shelves.len(), 2);
        assert_eq!(state.shelves[0].title, "Featured Tracks");

        state.set_shelves(Vec::new());
        assert!(state.shelves.is_empty());
    }

    #[test]
    fn test_selected_track_bounds_empty_state() {
        let state = SearchState::new();
        assert_eq!(state.selected_track(0), None);
        assert_eq!(state.selected_track(1), None);
        assert_eq!(state.selected_track(usize::MAX), None);
    }

    #[test]
    fn test_selected_track_bounds_out_of_range() {
        let mut state = SearchState::new();
        state.set_shelves(canned_shelves());

        // Canned shelves contain exactly 3 tracks across 2 shelves.
        assert!(state.selected_track(0).is_some());
        assert_eq!(state.selected_track(0).unwrap().id, "canned-1");
        assert!(state.selected_track(1).is_some());
        assert_eq!(state.selected_track(1).unwrap().id, "canned-2");
        assert!(state.selected_track(2).is_some());
        assert_eq!(state.selected_track(2).unwrap().id, "canned-3");

        // Out of bounds cases.
        assert_eq!(state.selected_track(3), None);
        assert_eq!(state.selected_track(4), None);
        assert_eq!(state.selected_track(999), None);
        assert_eq!(state.selected_track(usize::MAX), None);
    }

    #[test]
    fn test_flatten_row_contents_match_domain_data() {
        let shelves = vec![
            Shelf {
                title: "Rock Hits".to_owned(),
                items: vec![
                    SearchResult::Track(Track::new("t1", "Song A", "Artist 1").with_duration(185)),
                    SearchResult::Track(Track::new("t2", "Song B", "Artist 2").with_duration(200)),
                ],
            },
            Shelf {
                title: "Jazz Classics".to_owned(),
                items: vec![SearchResult::Track(Track::new("t3", "Song C", "Artist 3"))],
            },
        ];

        let mut state = SearchState::new();
        state.set_shelves(shelves);

        let rows = state.shelf_tracks();
        assert_eq!(rows.len(), 3);

        assert_eq!(rows[0].title, "Song A");
        assert_eq!(rows[0].artist, "Artist 1");
        assert_eq!(rows[0].duration, "3:05");

        assert_eq!(rows[1].title, "Song B");
        assert_eq!(rows[1].artist, "Artist 2");
        assert_eq!(rows[1].duration, "3:20");

        assert_eq!(rows[2].title, "Song C");
        assert_eq!(rows[2].artist, "Artist 3");
        assert_eq!(rows[2].duration, "");
    }

    #[test]
    fn test_flatten_row_contents_filters_non_tracks() {
        let shelves = vec![Shelf {
            title: "Mixed Items".to_owned(),
            items: vec![
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)),
                SearchResult::Track(
                    Track::new("t1", "Only Track", "Solo Artist").with_duration(120),
                ),
                SearchResult::Artist(Artist::new("ar1", "Artist Name", None)),
                SearchResult::Playlist(Playlist::new("p1", "Playlist 1", 10, None)),
            ],
        }];

        let mut state = SearchState::new();
        state.set_shelves(shelves);

        let rows = state.shelf_tracks();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Only Track");
        assert_eq!(rows[0].artist, "Solo Artist");
        assert_eq!(rows[0].duration, "2:00");

        assert_eq!(state.selected_track(0).unwrap().id, "t1");
        assert_eq!(state.selected_track(1), None);
    }

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
    fn test_sync_infra_catalog_stub_home_shelves() {
        let stub = CatalogStub::new();
        let shelves = stub.home_shelves();

        let mut state = SearchState::new();
        state.set_shelves(shelves);

        let rows = state.shelf_tracks();
        // CatalogStub produces home shelves with items.
        for (i, row) in rows.iter().enumerate() {
            let track = state.selected_track(i).expect("track must exist for row");
            assert_eq!(row.title, track.title);
            assert_eq!(row.artist, track.artist);
            assert_eq!(row.duration, format_duration(track.duration_secs));
        }
    }

    #[test]
    fn test_canned_shelves_structure() {
        let state = SearchState::with_canned_data();
        assert_eq!(state.shelves.len(), 2);
        assert_eq!(state.shelf_tracks().len(), 3);
        assert!(state.selected_track(0).is_some());
        assert!(state.selected_track(1).is_some());
        assert!(state.selected_track(2).is_some());
        assert!(state.selected_track(3).is_none());
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
    fn test_search_state_filter_select_deselect() {
        let shelves = vec![Shelf {
            title: "Mixed Results".to_owned(),
            items: vec![
                SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)),
                SearchResult::Artist(Artist::new("ar1", "Artist 1", None)),
                SearchResult::Playlist(Playlist::new("p1", "Playlist 1", 5, None)),
            ],
        }];

        let mut state = SearchState::new();
        state.set_shelves(shelves);

        assert_eq!(state.filter(), SearchFilter::All);
        assert_eq!(state.search_rows().len(), 4);

        state.set_filter(SearchFilter::Tracks);
        assert_eq!(state.filter(), SearchFilter::Tracks);
        let track_rows = state.search_rows();
        assert_eq!(track_rows.len(), 1);
        assert_eq!(track_rows[0].kind, "track");

        state.set_filter(SearchFilter::Albums);
        let album_rows = state.search_rows();
        assert_eq!(album_rows.len(), 1);
        assert_eq!(album_rows[0].kind, "album");

        state.set_filter(SearchFilter::Artists);
        let artist_rows = state.search_rows();
        assert_eq!(artist_rows.len(), 1);
        assert_eq!(artist_rows[0].kind, "artist");

        state.set_filter(SearchFilter::Playlists);
        let playlist_rows = state.search_rows();
        assert_eq!(playlist_rows.len(), 1);
        assert_eq!(playlist_rows[0].kind, "playlist");

        state.set_filter(SearchFilter::All);
        assert_eq!(state.search_rows().len(), 4);
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

    #[test]
    fn test_search_state_filtered_selection_out_of_range() {
        let shelves = vec![Shelf {
            title: "Mixed".to_owned(),
            items: vec![
                SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)),
                SearchResult::Track(Track::new("t2", "Track 2", "Artist 2")),
            ],
        }];

        let mut state = SearchState::new();
        state.set_shelves(shelves);

        state.set_filter(SearchFilter::Albums);
        assert_eq!(
            state.selected_item(0).map(|i| i.display()),
            Some("Album 1 - Artist 1".to_string())
        );
        assert_eq!(state.selected_item(1), None);
        assert_eq!(state.selected_item(2), None);
        assert_eq!(state.selected_item(999), None);
        assert_eq!(state.selected_item(usize::MAX), None);

        state.set_filter(SearchFilter::Playlists);
        assert_eq!(state.selected_item(0), None);
        assert_eq!(state.selected_item(1), None);
    }

    #[test]
    fn test_search_row_from_parts() {
        let parts = SearchRowParts {
            id: "id-1".into(),
            kind: "track".into(),
            title: "Title".into(),
            subtitle: "Artist".into(),
            album: "Album".into(),
            duration: "3:45".into(),
        };
        let row = SearchRow::from_parts(parts).with_artwork("https://artwork.local/test");
        assert_eq!(row.id, "id-1");
        assert_eq!(row.kind, "track");
        assert_eq!(row.title, "Title");
        assert_eq!(row.subtitle, "Artist");
        assert_eq!(row.album, "Album");
        assert_eq!(row.duration, "3:45");
        assert_eq!(row.artwork_url.as_deref(), Some("https://artwork.local/test"));
    }
}
