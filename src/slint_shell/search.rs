//! Slint search page state and view-model bridge.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, and zero async runtimes.
//!
//! Async boundary note: In future tranches, search queries and home shelves will be
//! fetched asynchronously from the Catalog port adapter. This sync bridge provides
//! the pure in-memory boundary holding query text, domain shelves, and flattened
//! display models.

use crate::domain::{SearchResult, Shelf, Track};

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
}

impl SearchState {
    /// Creates an empty search state.
    pub fn new() -> Self {
        Self { query: String::new(), shelves: Vec::new() }
    }

    /// Creates a search state populated with canned shelves for UI preview.
    pub fn with_canned_data() -> Self {
        Self { query: String::new(), shelves: canned_shelves() }
    }

    /// Updates the search query text.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    /// Updates the loaded catalog shelves.
    pub fn set_shelves(&mut self, shelves: Vec<Shelf>) {
        self.shelves = shelves;
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
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1")),
                SearchResult::Track(
                    Track::new("t1", "Only Track", "Solo Artist").with_duration(120),
                ),
                SearchResult::Artist(Artist::new("ar1", "Artist Name")),
                SearchResult::Playlist(Playlist::new("p1", "Playlist 1", 10)),
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
}
