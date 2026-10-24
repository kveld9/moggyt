//! Catalog library models: albums, artists, playlists, shelves, and pagination.

use super::search::{SearchFilter, Track};

/// Catalog album hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
}

impl Album {
    pub fn new(id: &str, title: &str, artist: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), artist: artist.to_owned() }
    }

    pub fn display(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }
}

/// Catalog artist hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

impl Artist {
    pub fn new(id: &str, name: &str) -> Self {
        Self { id: id.to_owned(), name: name.to_owned() }
    }

    pub fn display(&self) -> String {
        self.name.clone()
    }
}

/// Catalog playlist hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub track_count: usize,
}

impl Playlist {
    pub fn new(id: &str, title: &str, track_count: usize) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), track_count }
    }

    pub fn display(&self) -> String {
        format!("{} ({} tracks)", self.title, self.track_count)
    }
}

/// Unified search hit across catalog item types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchResult {
    Track(Track),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
}

impl SearchResult {
    /// Returns the matching category filter for this result.
    pub fn kind(&self) -> SearchFilter {
        match self {
            Self::Track(_) => SearchFilter::Tracks,
            Self::Album(_) => SearchFilter::Albums,
            Self::Artist(_) => SearchFilter::Artists,
            Self::Playlist(_) => SearchFilter::Playlists,
        }
    }

    /// One-line human-facing display string.
    pub fn display(&self) -> String {
        match self {
            Self::Track(track) => track.display(),
            Self::Album(album) => album.display(),
            Self::Artist(artist) => artist.display(),
            Self::Playlist(playlist) => playlist.display(),
        }
    }
}

/// Named shelf grouping catalog results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<SearchResult>,
}

impl Shelf {
    pub fn new(title: &str) -> Self {
        Self { title: title.to_owned(), items: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

/// Paginated container for catalog query results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

impl<T> SearchPage<T> {
    pub fn new(items: Vec<T>, total: usize, offset: usize, limit: usize) -> Self {
        Self { items, total, offset, limit }
    }

    pub fn page_items(&self) -> &[T] {
        &self.items
    }

    pub fn has_more(&self) -> bool {
        self.offset.saturating_add(self.items.len()) < self.total
    }
}

/// Filters search results by the selected category.
pub fn apply_filter(items: &[SearchResult], filter: SearchFilter) -> Vec<SearchResult> {
    if filter == SearchFilter::All {
        return items.to_vec();
    }
    items.iter().filter(|item| item.kind() == filter).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_kinds_and_displays() {
        let cases: [(SearchResult, SearchFilter, &str); 4] = [
            (
                SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                SearchFilter::Tracks,
                "Track 1 - Artist 1",
            ),
            (
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1")),
                SearchFilter::Albums,
                "Album 1 - Artist 1",
            ),
            (
                SearchResult::Artist(Artist::new("ar1", "Artist 1")),
                SearchFilter::Artists,
                "Artist 1",
            ),
            (
                SearchResult::Playlist(Playlist::new("p1", "Playlist 1", 10)),
                SearchFilter::Playlists,
                "Playlist 1 (10 tracks)",
            ),
        ];

        for (item, expected_kind, expected_display) in cases {
            assert_eq!(item.kind(), expected_kind);
            assert_eq!(item.display(), expected_display);
        }
    }

    #[test]
    fn shelf_len_and_emptiness() {
        let mut shelf = Shelf::new("Top Results");
        assert!(shelf.is_empty());
        assert_eq!(shelf.len(), 0);

        shelf.items.push(SearchResult::Artist(Artist::new("ar1", "Artist 1")));
        assert!(!shelf.is_empty());
        assert_eq!(shelf.len(), 1);
    }

    #[test]
    fn apply_filter_cases() {
        let items = vec![
            SearchResult::Track(Track::new("t1", "T1", "A1")),
            SearchResult::Album(Album::new("a1", "A1", "A1")),
            SearchResult::Artist(Artist::new("ar1", "A1")),
            SearchResult::Playlist(Playlist::new("p1", "P1", 5)),
        ];

        let cases = [
            (SearchFilter::All, 4),
            (SearchFilter::Tracks, 1),
            (SearchFilter::Albums, 1),
            (SearchFilter::Artists, 1),
            (SearchFilter::Playlists, 1),
        ];

        for (filter, expected_count) in cases {
            let filtered = apply_filter(&items, filter);
            assert_eq!(filtered.len(), expected_count);
            if filter != SearchFilter::All {
                assert!(filtered.iter().all(|item| item.kind() == filter));
            }
        }

        assert!(apply_filter(&[], SearchFilter::All).is_empty());
        assert!(apply_filter(&[], SearchFilter::Tracks).is_empty());
    }

    #[test]
    fn search_page_has_more_edges() {
        let cases = [
            (10, 10, 0, false),
            (5, 10, 0, true),
            (5, 10, 5, false),
            (0, 0, 0, false),
            (0, 10, 12, false),
        ];

        for (len, total, offset, expected_has_more) in cases {
            let items = vec![0; len];
            let page = SearchPage::new(items, total, offset, 10);
            assert_eq!(page.page_items().len(), len);
            assert_eq!(page.has_more(), expected_has_more);
        }
    }
}
