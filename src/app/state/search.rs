//! Search submissions, lyrics lookup, and playlist importing for [`AppState`].

use crate::app::{AppState, Page};
use crate::domain::{DetailKind, LyricsSnippet, SearchQuery, Track, extract_playlist_id};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Enqueues a single track at the tail of the playback queue.
    pub fn enqueue_track(&mut self, track: Track) {
        self.queue.push(track);
    }

    /// Imports a playlist from a URL and enqueues all tracks to the playback queue.
    ///
    /// Resolves the canonical playlist identifier, fetches playlist detail from the
    /// catalog, and enqueues tracks through the queue-mutation choke point. Returns
    /// the number of enqueued tracks. Empty-track playlists enqueue zero tracks and
    /// report zero without error. Parse and lookup failures return loud errors.
    pub fn import_playlist_url(&mut self, url: &str) -> anyhow::Result<usize> {
        let playlist_id = extract_playlist_id(url)
            .ok_or_else(|| anyhow::anyhow!("invalid or unsupported playlist URL: '{url}'"))?;
        let detail = self
            .catalog
            .detail(&playlist_id, DetailKind::Playlist)
            .ok_or_else(|| anyhow::anyhow!("playlist detail not found: '{playlist_id}'"))?;
        let count = detail.tracks.len();
        for track in detail.tracks {
            self.enqueue_track(track);
        }
        Ok(count)
    }

    /// Handles a search submission: routes to lyrics search when lyrics mode is active,
    /// otherwise parses standard query, queries the catalog, and enqueues the first hit.
    pub fn submit_search(&mut self) {
        if self.lyrics_mode {
            self.submit_lyrics_search();
            return;
        }
        self.search_error = None;
        self.lyrics_error = None;
        let Some(query) = SearchQuery::parse(&self.search_text) else {
            return;
        };
        let hits = self.catalog.search(query.text());
        self.search_results = hits.clone();
        if let Some(track) = hits.into_iter().next() {
            self.queue.push(track);
            self.page = Page::Queue;
        }
    }

    /// Submits a lyrics-based search query using `search_text`.
    /// Surfaces validation errors on blank or overlong input without panicking.
    pub fn submit_lyrics_search(&mut self) {
        let snippet = match LyricsSnippet::parse(&self.search_text) {
            Ok(s) => s,
            Err(err) => return self.record_lyrics_error(err),
        };
        self.search_error = None;
        self.lyrics_error = None;
        let hits = self.catalog.search_by_lyrics(&snippet);
        self.lyrics_results = hits.clone();
        self.search_results = hits.clone();
        if let Some(track) = hits.into_iter().next() {
            self.mutate_queue(|q| q.push(track));
            self.page = Page::Queue;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{ImportTestingCatalog, wired};
    use super::*;
    use crate::domain::{Album, Artist, Playlist, SearchResult, Track};
    use crate::infra::{AudioStub, LrclibStub, MemoryCoverCache, MemorySessionStore};

    fn shelf_item_display(item: &SearchResult) -> String {
        match item {
            SearchResult::Track(track) => format!("{} - {}", track.title, track.artist),
            SearchResult::Album(album) => {
                let subtitle = if album.artist.is_empty() { "Album" } else { &album.artist };
                format!("{} - {subtitle}", album.title)
            }
            SearchResult::Artist(artist) => format!("{} - Artist", artist.name),
            SearchResult::Playlist(playlist) => format!("{} - Playlist", playlist.title),
        }
    }

    #[test]
    fn empty_query_keeps_page_and_queue() {
        let mut state = wired();
        state.search_text = "   ".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Search);
        assert_eq!(state.queue.len(), 0);
    }

    #[test]
    fn search_hit_enqueues_and_navigates_to_queue() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Queue);
        assert_eq!(state.queue.len(), 1);
    }

    #[test]
    fn shelf_item_display_formatting() {
        assert_eq!(
            shelf_item_display(&SearchResult::Track(Track::new("t1", "TrackTitle", "TrackArtist"))),
            "TrackTitle - TrackArtist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Album(Album::new("a1", "AlbumTitle", "AlbumArtist"))),
            "AlbumTitle - AlbumArtist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Album(Album::new("a2", "AlbumTitle", ""))),
            "AlbumTitle - Album"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Artist(Artist::new("ar1", "ArtistName"))),
            "ArtistName - Artist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Playlist(Playlist::new("p1", "PlaylistTitle", 10))),
            "PlaylistTitle - Playlist"
        );
    }

    #[test]
    fn import_playlist_url_enqueues_stub_detail_tracks_and_returns_count() {
        let mut state = wired();
        let count = state
            .import_playlist_url("https://music.youtube.com/playlist?list=PLstub123")
            .expect("import succeeds");
        assert_eq!(count, 2);
        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.tracks()[0].id, "PLstub123-1");
        assert_eq!(state.queue.tracks()[1].id, "PLstub123-2");
    }

    #[test]
    fn import_playlist_url_unknown_id_errors_loudly() {
        let mut state = AppState::new(
            ImportTestingCatalog::default(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        let result =
            state.import_playlist_url("https://music.youtube.com/playlist?list=PLunknown_456");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("playlist detail not found"));
        assert!(state.queue.is_empty());
    }

    #[test]
    fn import_playlist_url_blank_url_errors() {
        let mut state = wired();
        assert!(state.import_playlist_url("").is_err());
        assert!(state.import_playlist_url("   ").is_err());
        assert!(state.queue.is_empty());
    }

    #[test]
    fn import_playlist_url_invalid_url_leaves_queue_untouched() {
        let mut state = wired();
        state.queue.push(Track::new("pre-existing", "Title", "Artist"));
        assert_eq!(state.queue.len(), 1);

        assert!(state.import_playlist_url("https://spotify.com/playlist?list=PL123").is_err());
        assert_eq!(state.queue.len(), 1);

        assert!(
            state.import_playlist_url("https://music.youtube.com/playlist?list=BADPREFIX").is_err()
        );
        assert_eq!(state.queue.len(), 1);

        assert!(state.import_playlist_url("ftp://music.youtube.com/playlist?list=PL123").is_err());
        assert_eq!(state.queue.len(), 1);

        assert_eq!(state.queue.tracks()[0].id, "pre-existing");
    }

    #[test]
    fn import_playlist_url_empty_tracks_enqueues_zero_and_reports_zero() {
        let mut state = AppState::new(
            ImportTestingCatalog::default(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        let count = state
            .import_playlist_url("https://music.youtube.com/playlist?list=PLempty_999")
            .expect("empty playlist succeeds");
        assert_eq!(count, 0);
        assert!(state.queue.is_empty());
    }
}
