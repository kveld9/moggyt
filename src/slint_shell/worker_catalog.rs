//! Blocking catalog search executors run on the playback worker thread.

use std::sync::{Arc, Mutex};

use crate::domain::{Detail, DetailKind, LyricsSnippet, SearchQuery, SearchResult, Shelf};
use crate::i18n::{Locale, tr};
use crate::infra::{Catalog, CatalogError};

/// Pages followed for an artist songs list (the catalog returns about 100 tracks per page).
const ARTIST_SONGS_MAX_PAGES: usize = 3;

/// Routes a lyrics-fragment query to the lyrics resolver, any other query to the catalog.
pub fn resolve_search<C: Catalog>(
    catalog: &C,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, CatalogError> {
    if !query.is_lyrics() {
        return catalog.search_filtered(query);
    }
    let snippet = LyricsSnippet::parse(query.text()).map_err(|_| CatalogError::Transport)?;
    catalog
        .search_by_lyrics(&snippet)
        .map(|tracks| tracks.into_iter().map(SearchResult::Track).collect())
}

/// Executes blocking search request off the UI thread and wraps results in a shelf.
pub fn execute_search<C: Catalog>(
    generation: u64,
    query: &SearchQuery,
    catalog: &Arc<Mutex<C>>,
    locale: Locale,
    songs_only: bool,
) -> (u64, String, Result<Vec<Shelf>, CatalogError>) {
    let res = catalog
        .lock()
        .map_err(|_| CatalogError::Transport)
        .and_then(|c| resolve_search(&*c, query));
    let shelves = res.map(|mut items| {
        items = crate::domain::apply_result_policy(items, songs_only);
        let mut shelf = Shelf::new(tr(locale, "Search Results"));
        shelf.items = items;
        vec![shelf]
    });
    (generation, query.text().to_string(), shelves)
}

/// Loads a detail view and attaches the complete songs list to artist pages.
///
/// The extra songs request is best effort: when the list is unavailable the popular
/// shelf stays playable and the degradation is logged as a warning.
pub fn fetch_detail<C: Catalog>(
    browse_id: &str,
    kind: DetailKind,
    catalog: &Arc<Mutex<C>>,
) -> Result<Option<Detail>, CatalogError> {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let guard = catalog.lock().map_err(|_| CatalogError::Transport)?;
    let detail = guard.detail(trimmed, kind)?;
    Ok(attach_artist_songs(&*guard, detail, kind))
}

/// Returns `detail` with the full artist songs list attached as its playback list.
fn attach_artist_songs<C: Catalog>(
    catalog: &C,
    detail: Option<Detail>,
    kind: DetailKind,
) -> Option<Detail> {
    let detail = detail?;
    if kind != DetailKind::Artist {
        return Some(detail);
    }
    let Some(songs_id) = detail.artist.as_ref().and_then(|page| page.songs_more_id.clone()) else {
        return Some(detail);
    };
    let songs = catalog.detail_paginated(&songs_id, DetailKind::Playlist, ARTIST_SONGS_MAX_PAGES);
    let tracks = match songs {
        Ok(Some(songs)) => songs.tracks,
        Ok(None) => Vec::new(),
        Err(err) => {
            tracing::warn!(%songs_id, %err, "artist songs list unavailable; using the shelf");
            Vec::new()
        }
    };
    Some(detail.with_queue_tracks(tracks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ArtistPage, Track};
    use crate::slint_shell::test_support::MockLibraryCatalog;

    fn artist_detail(tracks: Vec<Track>) -> Detail {
        let page = ArtistPage {
            songs_title: "Popular songs".to_owned(),
            songs_more_id: Some("VLsongs".to_owned()),
            sections: Vec::new(),
        };
        Detail::new(DetailKind::Artist, "Artist", "", tracks).with_artist_page(page)
    }

    #[test]
    fn fetch_detail_attaches_the_complete_artist_songs_list() {
        let shelf = vec![Track::new("t1", "One", "Artist")];
        let songs = Detail::new(
            DetailKind::Playlist,
            "Top songs",
            "",
            vec![Track::new("t1", "One", "Artist"), Track::new("t2", "Two", "Artist")],
        );
        let catalog = MockLibraryCatalog::default()
            .with_detail_by_id("UCartist", artist_detail(shelf))
            .with_detail_by_id("VLsongs", songs);
        let catalog = Arc::new(Mutex::new(catalog));

        let detail = fetch_detail("UCartist", DetailKind::Artist, &catalog);

        let detail = detail.expect("detail ok").expect("detail present");
        let ids: Vec<&str> = detail.playback_tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2"]);
        assert_eq!(detail.tracks.len(), 1, "the shelf still feeds the page row list");
    }

    #[test]
    fn fetch_detail_follows_artist_songs_pages_within_the_cap() {
        let shelf = vec![Track::new("t1", "One", "Artist")];
        let songs = Detail::new(
            DetailKind::Playlist,
            "Top songs",
            "",
            vec![
                Track::new("t1", "One", "Artist"),
                Track::new("t2", "Two", "Artist"),
                Track::new("t3", "Three", "Artist"),
            ],
        );
        let catalog = MockLibraryCatalog::default()
            .with_detail_by_id("UCartist", artist_detail(shelf))
            .with_paginated_detail_by_id("VLsongs", songs);
        let catalog = Arc::new(Mutex::new(catalog));

        let detail = fetch_detail("UCartist", DetailKind::Artist, &catalog);

        let detail = detail.expect("detail ok").expect("detail present");
        let ids: Vec<&str> = detail.playback_tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2", "t3"]);
        let calls = catalog.lock().expect("catalog lock").paginated_calls.clone();
        let calls = calls.lock().expect("calls lock").clone();
        assert_eq!(calls, vec![("VLsongs".to_owned(), ARTIST_SONGS_MAX_PAGES)]);
    }

    #[test]
    fn fetch_detail_keeps_the_popular_shelf_when_the_songs_list_is_missing() {
        let shelf = vec![Track::new("t1", "One", "Artist")];
        let catalog =
            MockLibraryCatalog::default().with_detail_by_id("UCartist", artist_detail(shelf));
        let catalog = Arc::new(Mutex::new(catalog));

        let detail = fetch_detail("UCartist", DetailKind::Artist, &catalog);

        let detail = detail.expect("detail ok").expect("detail present");
        let ids: Vec<&str> = detail.playback_tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1"]);
        assert!(detail.queue_tracks.is_none());
    }

    #[test]
    fn fetch_detail_does_not_request_songs_for_non_artist_details() {
        let album =
            Detail::new(DetailKind::Album, "Album", "", vec![Track::new("a1", "Track", "Artist")]);
        let catalog = MockLibraryCatalog::default().with_detail_by_id("MPREalbum", album);
        let catalog = Arc::new(Mutex::new(catalog));

        let detail = fetch_detail("MPREalbum", DetailKind::Album, &catalog);

        let detail = detail.expect("detail ok").expect("detail present");
        assert!(detail.queue_tracks.is_none());
        assert_eq!(detail.playback_tracks().len(), 1);
    }

    #[test]
    fn fetch_detail_blank_browse_id_returns_none() {
        let catalog = Arc::new(Mutex::new(MockLibraryCatalog::default()));

        let detail = fetch_detail("   ", DetailKind::Artist, &catalog);

        assert!(detail.expect("no transport error").is_none());
    }

    #[test]
    fn test_resolve_search_plain_query_delegates_to_catalog() {
        let catalog = MockLibraryCatalog::default();
        let query = SearchQuery::parse("test").expect("valid query");
        let results = resolve_search(&catalog, &query).expect("results ok");
        assert!(results.is_empty());
    }

    #[test]
    fn test_execute_search_ranks_songs_ahead_of_music_videos() {
        use crate::domain::{MusicVideoType, Track};

        let video = SearchResult::Track(
            Track::new("v1", "Video", "Artist").with_video_type(MusicVideoType::Omv),
        );
        let song = SearchResult::Track(
            Track::new("s1", "Song", "Artist").with_video_type(MusicVideoType::Atv),
        );
        let catalog = MockLibraryCatalog::default()
            .with_search_filtered(Ok(vec![video.clone(), song.clone()]));
        let cat = Arc::new(Mutex::new(catalog));
        let query = SearchQuery::parse("zzz").expect("valid query");

        let (_, _, shelves) = execute_search(1, &query, &cat, Locale::English, false);

        assert_eq!(shelves.expect("shelves ok")[0].items, vec![song, video]);
    }
}
