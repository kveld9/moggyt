//! Cover art background fetching, decoding pipeline, and image synchronization.
//!
//! Hexagonal invariant: asynchronous background decoding off UI thread with thread-safe
//! Image construction on the event loop.

use std::collections::{HashMap, HashSet};

use crate::domain::{SearchResult, Shelf};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};
pub(crate) use crate::slint_shell::cover_pool::spawn_cover_fetch_pool;
use crate::slint_shell::playback_worker::{PlaybackWorkerCommand, thumbnail_variant_for_row};
use crate::slint_shell::state::SlintAppState;

/// Helper extracting artwork URLs across shelves for pre-fetching.
pub(crate) fn iter_shelf_artwork_urls(shelves: &[Shelf]) -> impl Iterator<Item = &str> {
    shelves.iter().flat_map(|s| s.items.iter()).filter_map(|it| match it {
        SearchResult::Track(t) => t.artwork.as_deref(),
        SearchResult::Album(a) => a.artwork.as_deref(),
        SearchResult::Artist(a) => a.artwork.as_deref(),
        SearchResult::Playlist(p) => p.artwork.as_deref(),
    })
}

/// Resolves cached cover image for a shelf or queue row, routed through thumbnail_variant_for_row.
pub(crate) fn resolve_row_cover_image(
    artwork_url: Option<&str>,
    covers: &HashMap<String, slint::Image>,
) -> slint::Image {
    let Some(url) = artwork_url else {
        return slint::Image::default();
    };
    let thumb = thumbnail_variant_for_row(url);
    covers.get(&thumb).or_else(|| covers.get(url)).cloned().unwrap_or_default()
}

/// Dispatches a single cover fetch command with explicit generation if not already in UI cache or pending set.
pub(crate) fn dispatch_cover_with_generation_if_needed(
    url: &str,
    generation: u64,
    covers: &HashMap<String, slint::Image>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    seen: &mut HashSet<String>,
) {
    let row_url = thumbnail_variant_for_row(url);
    if covers.contains_key(&row_url) || !seen.insert(row_url.clone()) {
        return;
    }
    let _ = worker_tx.send(PlaybackWorkerCommand::FetchCover { url: row_url, generation });
}

/// Dispatches a single cover fetch command if not already in UI cache or pending set.
pub(crate) fn dispatch_shelf_cover_if_needed<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    url: &str,
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    seen: &mut HashSet<String>,
) {
    dispatch_cover_with_generation_if_needed(
        url,
        state.search_generation,
        &state.search_covers,
        worker_tx,
        seen,
    );
}

/// Requests background cover decoding for all uncached artwork URLs in shelves.
pub(crate) fn request_shelf_covers<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let mut seen = HashSet::new();
    for url in iter_shelf_artwork_urls(&state.home_shelves) {
        dispatch_shelf_cover_if_needed(url, state, worker_tx, &mut seen);
    }
}

/// Requests background cover decoding for all uncached artwork URLs in detail view.
pub fn request_detail_covers<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(detail) = &state.detail else {
        return;
    };
    let mut seen = HashSet::new();
    if let Some(url) = detail.artwork.as_deref() {
        dispatch_cover_with_generation_if_needed(
            url,
            state.detail_generation,
            &state.search_covers,
            worker_tx,
            &mut seen,
        );
    }
    for track in &detail.tracks {
        if let Some(url) = track.artwork.as_deref() {
            dispatch_cover_with_generation_if_needed(
                url,
                state.detail_generation,
                &state.search_covers,
                worker_tx,
                &mut seen,
            );
        }
    }
}

fn dispatch_library_cover_urls<
    'a,
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    urls: impl Iterator<Item = Option<&'a str>>,
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    seen: &mut HashSet<String>,
) {
    for url in urls.flatten() {
        dispatch_cover_with_generation_if_needed(
            url,
            state.library_generation,
            &state.search_covers,
            worker_tx,
            seen,
        );
    }
}

/// Requests background cover decoding for all uncached artwork URLs in user library.
pub fn request_library_covers<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let mut seen = HashSet::new();
    dispatch_library_cover_urls(
        state.library_playlists.iter().map(|p| p.artwork.as_deref()),
        state,
        worker_tx,
        &mut seen,
    );
    dispatch_library_cover_urls(
        state.library_albums.iter().map(|a| a.artwork.as_deref()),
        state,
        worker_tx,
        &mut seen,
    );
    dispatch_library_cover_urls(
        state.library_artists.iter().map(|ar| ar.artwork.as_deref()),
        state,
        worker_tx,
        &mut seen,
    );
}

/// Requests background cover decoding for all uncached artwork URLs in explore shelves.
pub fn request_explore_covers<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let mut seen = HashSet::new();
    for url in iter_shelf_artwork_urls(&state.explore_shelves) {
        dispatch_cover_with_generation_if_needed(
            url,
            state.explore_generation,
            &state.search_covers,
            worker_tx,
            &mut seen,
        );
    }
}

/// Requests background cover decoding for a track being enqueued if uncached.
pub fn request_queue_track_cover(
    artwork_url: Option<&str>,
    generation: u64,
    covers: &HashMap<String, slint::Image>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(url) = artwork_url else {
        return;
    };
    let mut seen = HashSet::new();
    dispatch_cover_with_generation_if_needed(url, generation, covers, worker_tx, &mut seen);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::domain::{Album, Artist, Detail, DetailKind, Playlist, Track};
    use crate::infra::MemoryCoverCache;
    use crate::slint_shell::detail::{detail_has_artwork, resolve_detail_cover_url};
    use crate::slint_shell::platform::rgba_to_image;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_cover_pipeline_store_and_lookup_key_roundtrip() {
        let cache = Arc::new(Mutex::new(MemoryCoverCache::new()));
        let url = "https://example.com/artwork.jpg";
        cache.lock().unwrap().put(url, vec![1, 2, 3]);

        assert_eq!(cache.lock().unwrap().get(url), Some(vec![1, 2, 3]));

        let track = Track::new("t1", "Title", "Artist").with_artwork(url);
        let lookup_key = track.artwork.as_ref();
        assert_eq!(lookup_key.map(String::as_str), Some(url));
        assert_eq!(lookup_key.and_then(|u| cache.lock().unwrap().get(u)), Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_resolve_row_cover_image_routes_through_thumbnail_variant() {
        let covers = HashMap::new();
        let default_img = resolve_row_cover_image(None, &covers);
        assert_eq!(default_img.size(), slint::Image::default().size());

        let url = "https://lh3.googleusercontent.com/test-thumb-large";
        let thumb_url = thumbnail_variant_for_row(url);
        assert_eq!(thumb_url, "https://lh3.googleusercontent.com/test-thumb-small");

        let missing = resolve_row_cover_image(Some(url), &covers);
        assert_eq!(missing.size(), slint::Image::default().size());
    }

    #[test]
    fn test_detail_cover_sync_resolution_matrix() {
        let dummy_img = rgba_to_image(&[255, 0, 0, 255], 1, 1).unwrap();
        let header_url = "https://img.local/header.jpg";
        let track_url = "https://img.local/track.jpg";

        let t1 = Track::new("t1", "Track 1", "Artist 1").with_artwork(track_url);
        let detail_with_art = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![t1.clone()])
            .with_artwork(header_url);
        assert!(detail_has_artwork(&detail_with_art));

        let mut covers = HashMap::new();
        covers.insert(track_url.to_string(), dummy_img.clone());
        let cover_uncached = resolve_detail_cover_url(&detail_with_art)
            .map(|url| resolve_row_cover_image(Some(url), &covers))
            .unwrap_or_default();
        assert_eq!(cover_uncached.size(), slint::Image::default().size());

        covers.insert(header_url.to_string(), dummy_img.clone());
        let cover_cached = resolve_detail_cover_url(&detail_with_art)
            .map(|url| resolve_row_cover_image(Some(url), &covers))
            .unwrap_or_default();
        assert_eq!(cover_cached.size(), dummy_img.size());

        let detail_no_art = Detail::new(DetailKind::Playlist, "Title", "Sub", vec![t1]);
        assert!(!detail_has_artwork(&detail_no_art));
        let cover_fallback = resolve_detail_cover_url(&detail_no_art)
            .map(|url| resolve_row_cover_image(Some(url), &covers))
            .unwrap_or_default();
        assert_eq!(cover_fallback.size(), dummy_img.size());

        let empty_covers = HashMap::new();
        let cover_fallback_uncached = resolve_detail_cover_url(&detail_no_art)
            .map(|url| resolve_row_cover_image(Some(url), &empty_covers))
            .unwrap_or_default();
        assert_eq!(cover_fallback_uncached.size(), slint::Image::default().size());
    }

    #[test]
    fn test_dispatch_shelf_cover_if_needed_routes_and_deduplicates() {
        let state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        let mut seen = HashSet::new();

        let input_url = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg";
        let expected_url = "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg";

        dispatch_shelf_cover_if_needed(input_url, &state, &tx, &mut seen);

        let cmd = rx.try_recv().expect("command dispatched");
        match cmd {
            PlaybackWorkerCommand::FetchCover { url, generation } => {
                assert_eq!(url, expected_url);
                assert_eq!(generation, state.search_generation);
            }
            _ => panic!("unexpected command variant"),
        }

        dispatch_shelf_cover_if_needed(input_url, &state, &tx, &mut seen);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_request_shelf_covers_dispatches_thumbnail_variants() {
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1")
            .with_artwork("https://lh3.googleusercontent.com/test-thumb-large");
        let track2 = Track::new("t2", "Track 2", "Artist 2")
            .with_artwork("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg");

        let mut shelf = Shelf::new("Shelf 1");
        shelf.items.push(SearchResult::Track(track1));
        shelf.items.push(SearchResult::Track(track2));
        state.home_shelves = vec![shelf];

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_shelf_covers(&state, &tx);

        let mut received_urls = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, .. } = cmd {
                received_urls.push(url);
            }
        }

        assert_eq!(received_urls.len(), 2);
        assert_eq!(received_urls[0], "https://lh3.googleusercontent.com/test-thumb-small");
        assert_eq!(received_urls[1], "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg");
    }

    #[test]
    fn test_request_shelf_covers_dispatches_all_search_result_kinds() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1")
            .with_artwork("https://lh3.googleusercontent.com/track-large");
        let album = Album::new(
            "a1",
            "Album 1",
            "Artist 1",
            Some("https://lh3.googleusercontent.com/album-large".to_owned()),
        );
        let artist = Artist::new(
            "ar1",
            "Artist 1",
            Some("https://lh3.googleusercontent.com/artist-thumb".to_owned()),
        );
        let playlist = Playlist::new(
            "p1",
            "Playlist 1",
            10,
            Some("https://i.ytimg.com/vi/playlist-id/hqdefault.jpg".to_owned()),
        );
        let no_art_album = Album::new("a2", "No Art", "Artist 2", None);

        let mut shelf = Shelf::new("All Kinds Shelf");
        shelf.items.push(SearchResult::Track(track));
        shelf.items.push(SearchResult::Album(album));
        shelf.items.push(SearchResult::Artist(artist));
        shelf.items.push(SearchResult::Playlist(playlist));
        shelf.items.push(SearchResult::Album(no_art_album));
        state.home_shelves = vec![shelf];

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_shelf_covers(&state, &tx);

        let mut received_urls = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, .. } = cmd {
                received_urls.push(url);
            }
        }

        assert_eq!(received_urls.len(), 4);
        assert_eq!(received_urls[0], "https://lh3.googleusercontent.com/track-small");
        assert_eq!(received_urls[1], "https://lh3.googleusercontent.com/album-small");
        assert_eq!(received_urls[2], "https://lh3.googleusercontent.com/artist-thumb");
        assert_eq!(received_urls[3], "https://i.ytimg.com/vi/playlist-id/mqdefault.jpg");
    }

    #[test]
    fn test_request_detail_covers_dispatches_with_detail_generation() {
        let mut state = test_wired();
        state.detail_generation = 42;
        let track1 = Track::new("t1", "Track 1", "Artist 1")
            .with_artwork("https://lh3.googleusercontent.com/detail-thumb-large");
        let track2 = Track::new("t2", "Track 2", "Artist 2")
            .with_artwork("https://i.ytimg.com/vi/abc/hqdefault.jpg");
        state.detail =
            Some(Detail::new(DetailKind::Album, "Album Title", "Artist", vec![track1, track2]));

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_detail_covers(&state, &tx);

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
                received.push((url, generation));
            }
        }

        assert_eq!(received.len(), 2);
        assert_eq!(
            received[0],
            ("https://lh3.googleusercontent.com/detail-thumb-small".to_string(), 42)
        );
        assert_eq!(received[1], ("https://i.ytimg.com/vi/abc/mqdefault.jpg".to_string(), 42));
    }

    #[test]
    fn test_request_detail_covers_dispatches_header_artwork_first() {
        let mut state = test_wired();
        state.detail_generation = 100;
        let track = Track::new("t1", "Track 1", "Artist 1")
            .with_artwork("https://lh3.googleusercontent.com/track-thumb-large");
        let detail = Detail::new(DetailKind::Playlist, "Playlist Title", "Creator", vec![track])
            .with_artwork("https://lh3.googleusercontent.com/header-thumb-large");
        state.detail = Some(detail);

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_detail_covers(&state, &tx);

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
                received.push((url, generation));
            }
        }

        assert_eq!(received.len(), 2);
        assert_eq!(
            received[0],
            ("https://lh3.googleusercontent.com/header-thumb-small".to_string(), 100)
        );
        assert_eq!(
            received[1],
            ("https://lh3.googleusercontent.com/track-thumb-small".to_string(), 100)
        );
    }

    #[test]
    fn test_request_library_covers_dispatches_all_entities() {
        let mut state = test_wired();
        state.library_generation = 7;
        state.library_playlists = vec![Playlist::new(
            "p1",
            "Play",
            10,
            Some("https://lh3.googleusercontent.com/p-thumb-large".into()),
        )];
        state.library_albums = vec![Album::new(
            "a1",
            "Alb",
            "Art",
            Some("https://lh3.googleusercontent.com/a-thumb-large".into()),
        )];
        state.library_artists = vec![Artist::new(
            "ar1",
            "ArtName",
            Some("https://lh3.googleusercontent.com/ar-thumb-large".into()),
        )];

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_library_covers(&state, &tx);

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
                received.push((url, generation));
            }
        }

        assert_eq!(received.len(), 3);
        assert_eq!(received[0], ("https://lh3.googleusercontent.com/p-thumb-small".to_string(), 7));
        assert_eq!(received[1], ("https://lh3.googleusercontent.com/a-thumb-small".to_string(), 7));
        assert_eq!(
            received[2],
            ("https://lh3.googleusercontent.com/ar-thumb-small".to_string(), 7)
        );
    }

    #[test]
    fn test_request_queue_track_cover_dispatches_with_play_generation() {
        let mut state = test_wired();
        state.play_generation = 99;
        let track = Track::new("t1", "Track 1", "Artist 1")
            .with_artwork("https://lh3.googleusercontent.com/q-thumb-large");

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_queue_track_cover(
            track.artwork.as_deref(),
            state.play_generation,
            &state.search_covers,
            &tx,
        );

        let cmd = rx.try_recv().expect("command dispatched");
        match cmd {
            PlaybackWorkerCommand::FetchCover { url, generation } => {
                assert_eq!(url, "https://lh3.googleusercontent.com/q-thumb-small");
                assert_eq!(generation, 99);
            }
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn test_request_explore_covers_dispatches_with_generation() {
        let mut state = test_wired();
        state.explore_generation = 15;
        let shelf = Shelf {
            title: "Charts".to_owned(),
            items: vec![
                SearchResult::Track(
                    Track::new("t1", "Track 1", "Artist 1")
                        .with_artwork("https://lh3.googleusercontent.com/track-large"),
                ),
                SearchResult::Album(Album::new(
                    "a1",
                    "Album 1",
                    "Artist 1",
                    Some("https://lh3.googleusercontent.com/album-large".to_owned()),
                )),
            ],
        };
        state.explore_shelves = vec![shelf];

        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        request_explore_covers(&state, &tx);

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
                received.push((url, generation));
            }
        }

        assert_eq!(received.len(), 2);
        assert_eq!(received[0], ("https://lh3.googleusercontent.com/track-small".to_string(), 15));
        assert_eq!(received[1], ("https://lh3.googleusercontent.com/album-small".to_string(), 15));
    }

    #[test]
    fn test_dispatch_library_cover_urls_skips_none() {
        let state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        let mut seen = HashSet::new();
        let urls = [None, Some("https://lh3.googleusercontent.com/test-thumb"), None];
        dispatch_library_cover_urls(urls.into_iter(), &state, &tx, &mut seen);

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, .. } = cmd {
                received.push(url);
            }
        }
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], "https://lh3.googleusercontent.com/test-thumb");
    }
}
