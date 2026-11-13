//! Catalog library models: albums, artists, playlists, shelves, and pagination.

use serde::{Deserialize, Serialize};

use super::search::{SearchFilter, Track};

/// Catalog album hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub artwork: Option<String>,
    /// Browse id of the credited artist when the catalog item carries one.
    #[serde(default)]
    pub artist_id: Option<String>,
    /// Release type label carried by the payload ("Single", "Album", "EP"), when present.
    #[serde(default)]
    pub release_type: Option<String>,
}

impl Album {
    pub fn new(id: &str, title: &str, artist: &str, artwork: Option<String>) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            artist: artist.to_owned(),
            artwork,
            artist_id: None,
            release_type: None,
        }
    }

    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        self.artwork = Some(artwork.into());
        self
    }

    pub fn with_artist_id(mut self, artist_id: Option<String>) -> Self {
        self.artist_id = artist_id;
        self
    }

    pub fn with_release_type(mut self, release_type: Option<String>) -> Self {
        self.release_type = release_type;
        self
    }

    pub fn display(&self) -> String {
        format!("{} - {}", self.title, self.artist)
    }

    /// Caption shown under the album title: release type, then the credited artist.
    pub fn caption(&self) -> String {
        let artist = self.artist.trim();
        match self.release_type.as_deref() {
            Some(kind) if !artist.is_empty() => format!("{kind} \u{2022} {artist}"),
            Some(kind) => kind.to_owned(),
            None => artist.to_owned(),
        }
    }
}

/// Catalog artist hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub artwork: Option<String>,
}

impl Artist {
    pub fn new(id: &str, name: &str, artwork: Option<String>) -> Self {
        Self { id: id.to_owned(), name: name.to_owned(), artwork }
    }

    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        self.artwork = Some(artwork.into());
        self
    }

    pub fn display(&self) -> String {
        self.name.clone()
    }
}

/// Catalog playlist hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub track_count: usize,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub params: Option<String>,
}

impl Playlist {
    pub fn new(id: &str, title: &str, track_count: usize, artwork: Option<String>) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), track_count, artwork, params: None }
    }

    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        self.artwork = Some(artwork.into());
        self
    }

    pub fn with_params(mut self, params: impl Into<String>) -> Self {
        let p = params.into();
        self.params = if p.trim().is_empty() { None } else { Some(p) };
        self
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

/// Category of detail container view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailKind {
    Album,
    Artist,
    Playlist,
}

/// Category of a card rendered inside an artist page section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailCardKind {
    Album,
    Artist,
}

/// Browsable card in an artist page section (album, single/EP, or similar artist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailCard {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<String>,
    pub kind: DetailCardKind,
}

impl DetailCard {
    pub fn new(
        id: &str,
        title: &str,
        subtitle: &str,
        artwork: Option<String>,
        kind: DetailCardKind,
    ) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            artwork,
            kind,
        }
    }
}

/// Named section of an artist page (Albums, Singles and EPs, similar artists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailSection {
    pub title: String,
    pub cards: Vec<DetailCard>,
}

/// Artist-specific detail payload carried by [`Detail::artist`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistPage {
    /// Server-provided heading for the most popular songs shelf.
    pub songs_title: String,
    /// Browse id of the full songs list ("Show all"); starts with `VL`.
    pub songs_more_id: Option<String>,
    /// Ordered carousel sections returned by the artist page.
    pub sections: Vec<DetailSection>,
}

/// Detailed view for an album, artist, or playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub kind: DetailKind,
    pub title: String,
    pub subtitle: String,
    pub description: Option<String>,
    pub artwork: Option<String>,
    pub tracks: Vec<Track>,
    /// Artist-page sections; `None` for albums and playlists.
    pub artist: Option<ArtistPage>,
}

impl Detail {
    pub fn new(kind: DetailKind, title: &str, subtitle: &str, tracks: Vec<Track>) -> Self {
        Self {
            kind,
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            description: None,
            artwork: None,
            tracks,
            artist: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        let desc = description.into();
        self.description = if desc.trim().is_empty() { None } else { Some(desc) };
        self
    }

    pub fn with_artwork(mut self, artwork: impl Into<String>) -> Self {
        let art = artwork.into();
        self.artwork = if art.trim().is_empty() { None } else { Some(art) };
        self
    }

    pub fn with_artist_page(mut self, page: ArtistPage) -> Self {
        self.artist = Some(page);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_construction_and_fields() {
        let tracks = vec![Track::new("t1", "Track 1", "Artist 1")];
        let detail = Detail::new(DetailKind::Album, "Title", "Subtitle", tracks.clone());
        assert_eq!(detail.kind, DetailKind::Album);
        assert_eq!(detail.title, "Title");
        assert_eq!(detail.subtitle, "Subtitle");
        assert_eq!(detail.description, None);
        assert_eq!(detail.artwork, None);
        assert_eq!(detail.tracks, tracks);
        assert_eq!(detail.artist, None);

        let enriched = detail
            .with_description("A collection of tracks.")
            .with_artwork("https://img.local/cover.jpg");
        assert_eq!(enriched.description.as_deref(), Some("A collection of tracks."));
        assert_eq!(enriched.artwork.as_deref(), Some("https://img.local/cover.jpg"));

        let trimmed_empty = enriched.with_description("   ").with_artwork("");
        assert_eq!(trimmed_empty.description, None);
        assert_eq!(trimmed_empty.artwork, None);
    }

    #[test]
    fn artist_page_construction_and_builder() {
        let page = ArtistPage {
            songs_title: "Popular songs".to_owned(),
            songs_more_id: Some("VL123".to_owned()),
            sections: vec![DetailSection {
                title: "Albums".to_owned(),
                cards: vec![DetailCard::new(
                    "MPREb_1",
                    "Album",
                    "2026",
                    Some("https://img.local/a.jpg".to_owned()),
                    DetailCardKind::Album,
                )],
            }],
        };
        let detail = Detail::new(DetailKind::Artist, "Artist", "", Vec::new())
            .with_artist_page(page.clone());
        assert_eq!(detail.artist, Some(page.clone()));
        assert_eq!(page.sections[0].cards[0].kind, DetailCardKind::Album);
        assert_eq!(DetailCardKind::Artist, DetailCardKind::Artist);
    }

    #[test]
    fn item_kinds_and_displays() {
        let cases: [(SearchResult, SearchFilter, &str); 4] = [
            (
                SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                SearchFilter::Tracks,
                "Track 1 - Artist 1",
            ),
            (
                SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)),
                SearchFilter::Albums,
                "Album 1 - Artist 1",
            ),
            (
                SearchResult::Artist(Artist::new("ar1", "Artist 1", None)),
                SearchFilter::Artists,
                "Artist 1",
            ),
            (
                SearchResult::Playlist(Playlist::new("p1", "Playlist 1", 10, None)),
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

        shelf.items.push(SearchResult::Artist(Artist::new("ar1", "Artist 1", None)));
        assert!(!shelf.is_empty());
        assert_eq!(shelf.len(), 1);
    }

    #[test]
    fn album_and_playlist_serde_roundtrip() {
        let album = Album::new(
            "a1",
            "Album Title",
            "Artist Name",
            Some("https://artwork.local/a1".to_owned()),
        );
        let album_json = serde_json::to_string(&album).expect("serialize album");
        let album_de: Album = serde_json::from_str(&album_json).expect("deserialize album");
        assert_eq!(album, album_de);

        let playlist =
            Playlist::new("p1", "Playlist Title", 42, Some("https://artwork.local/p1".to_owned()));
        let playlist_json = serde_json::to_string(&playlist).expect("serialize playlist");
        let playlist_de: Playlist =
            serde_json::from_str(&playlist_json).expect("deserialize playlist");
        assert_eq!(playlist, playlist_de);
    }

    #[test]
    fn album_with_artist_id_roundtrips() {
        let album = Album::new("a1", "Album", "Single", None);
        assert_eq!(album.artist_id, None);
        assert_eq!(album.display(), "Album - Single");

        let tagged = album.with_artist_id(Some("UC_artist".to_owned()));
        assert_eq!(tagged.artist_id.as_deref(), Some("UC_artist"));
        assert_eq!(tagged.display(), "Album - Single");
    }

    #[test]
    fn album_caption_combines_release_type_and_artist() {
        let album = Album::new("a1", "Call Me", "Toosii", None);
        assert_eq!(album.caption(), "Toosii");

        let single = album.with_release_type(Some("Single".to_owned()));
        assert_eq!(single.caption(), "Single \u{2022} Toosii");
        assert_eq!(single.display(), "Call Me - Toosii");
    }

    #[test]
    fn album_caption_handles_missing_parts() {
        let typed =
            Album::new("a1", "Call Me", "  ", None).with_release_type(Some("EP".to_owned()));
        assert_eq!(typed.caption(), "EP");

        let empty = Album::new("a2", "Call Me", " ", None);
        assert_eq!(empty.caption(), "");
    }

    #[test]
    fn artwork_round_trips_through_constructors_and_display_unaffected() {
        let album_none = Album::new("a1", "Album", "Artist", None);
        assert_eq!(album_none.artwork, None);
        assert_eq!(album_none.display(), "Album - Artist");
        let res_album_none = SearchResult::Album(album_none);
        assert_eq!(res_album_none.kind(), SearchFilter::Albums);
        assert_eq!(res_album_none.display(), "Album - Artist");

        let album_some = Album::new(
            "a2",
            "Album 2",
            "Artist 2",
            Some("https://artwork.local/album.jpg".to_owned()),
        );
        assert_eq!(album_some.artwork.as_deref(), Some("https://artwork.local/album.jpg"));
        assert_eq!(album_some.display(), "Album 2 - Artist 2");
        let res_album_some = SearchResult::Album(album_some);
        assert_eq!(res_album_some.kind(), SearchFilter::Albums);
        assert_eq!(res_album_some.display(), "Album 2 - Artist 2");

        let artist_none = Artist::new("ar1", "Artist Name", None);
        assert_eq!(artist_none.artwork, None);
        assert_eq!(artist_none.display(), "Artist Name");
        let res_artist_none = SearchResult::Artist(artist_none);
        assert_eq!(res_artist_none.kind(), SearchFilter::Artists);
        assert_eq!(res_artist_none.display(), "Artist Name");

        let artist_some = Artist::new(
            "ar2",
            "Artist Name 2",
            Some("https://artwork.local/artist.jpg".to_owned()),
        );
        assert_eq!(artist_some.artwork.as_deref(), Some("https://artwork.local/artist.jpg"));
        assert_eq!(artist_some.display(), "Artist Name 2");
        let res_artist_some = SearchResult::Artist(artist_some);
        assert_eq!(res_artist_some.kind(), SearchFilter::Artists);
        assert_eq!(res_artist_some.display(), "Artist Name 2");

        let playlist_none = Playlist::new("p1", "Playlist", 12, None);
        assert_eq!(playlist_none.artwork, None);
        assert_eq!(playlist_none.display(), "Playlist (12 tracks)");
        let res_playlist_none = SearchResult::Playlist(playlist_none);
        assert_eq!(res_playlist_none.kind(), SearchFilter::Playlists);
        assert_eq!(res_playlist_none.display(), "Playlist (12 tracks)");

        let playlist_some = Playlist::new(
            "p2",
            "Playlist 2",
            24,
            Some("https://artwork.local/playlist.jpg".to_owned()),
        );
        assert_eq!(playlist_some.artwork.as_deref(), Some("https://artwork.local/playlist.jpg"));
        assert_eq!(playlist_some.display(), "Playlist 2 (24 tracks)");
        let res_playlist_some = SearchResult::Playlist(playlist_some);
        assert_eq!(res_playlist_some.kind(), SearchFilter::Playlists);
        assert_eq!(res_playlist_some.display(), "Playlist 2 (24 tracks)");
    }

    #[test]
    fn playlist_with_params_stores_non_empty_trimmed() {
        assert_eq!(Playlist::new("p1", "P", 0, None).with_params("p").params.as_deref(), Some("p"));
        assert_eq!(Playlist::new("p2", "P", 0, None).with_params("   ").params, None);
    }
}
