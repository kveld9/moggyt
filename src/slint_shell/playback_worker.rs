//! Background playback worker and request/response dispatch off the Slint UI thread.
//!
//! # Ownership and Threading Model
//!
//! Slint UI thread owns the window and `SharedState` (`Rc<RefCell<SlintAppState>>`).
//! Because Slint requires UI state to remain on the main thread and `Rc<RefCell<..>>`
//! is `!Send`, all blocking adapter calls are decoupled from the Slint event loop:
//!
//! 1. **Audio backend**: Held in `Arc<Mutex<A>>`. `LiveAudio` (`RodioBackend`) is `Send`
//!    because `Sink`, `YtDlpResolver`, and rodio's `OutputStream` implement `Send`.
//!    Wrapping in `Mutex` makes it `Send + Sync`, allowing worker threads to call
//!    `audio.play()` (which performs stream resolution via yt-dlp and audio fetching)
//!    while the UI thread uses non-blocking `try_lock()` for 250ms periodic progress ticks.
//!
//! 2. **Catalog**: Held in `Arc<Mutex<C>>`. `LiveCatalog` wraps `reqwest::blocking::Client`,
//!    which is `Send + Sync`. Search and detail queries run on background worker threads,
//!    returning owned domain results (`Vec<Shelf>`, `Option<Detail>`) to the UI thread.
//!
//! 3. **Lyrics source**: Held in `Arc<L>`. `LrclibClient` is `Send + Sync`. Remote lookups
//!    run on the worker thread, returning owned `Vec<LyricLine>` data.
//!
//! 4. **Cover cache**: Held in `Arc<Mutex<K>>`. `DiskCoverCache` is `Send`. Remote artwork
//!    fetching (`fetch_artwork`) and CPU-intensive decoding (`decode_cover`) run entirely
//!    on the worker thread, crossing the thread boundary as owned `CoverArtData` (`Vec<u8>`).
//!    Slint `Image` textures are constructed strictly on the UI thread via `rgba_to_image`.
//!
//! 5. **Scrobbler**: Held in `Arc<Mutex<Box<dyn Scrobbler>>>`. `Scrobbler: Send`. HTTP posts
//!    to Last.fm run on the worker thread without stalling UI event processing.
//!
//! 6. **Generation counters**: Each asynchronous operation (play, search, detail, library)
//!    increments an atomic/state generation counter. Upon completion, the UI thread verifies
//!    that the completion's generation matches current state, discarding stale completions.

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use crate::domain::{Detail, DetailKind, SearchQuery, Shelf};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog, CoverCache, LyricLine,
    LyricOverrideDir, LyricsSource, Scrobbler, decode_cover, fetch_artwork,
};
use crate::slint_shell::platform::{CoverArtData, PresenceSnapshot};

/// Request payload dispatched to the playback worker thread.
#[derive(Debug, Clone)]
pub struct PlayTrackRequest {
    pub generation: u64,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub duration_secs: Option<u32>,
    pub hi_res_covers: bool,
    pub lyric_overrides: Option<LyricOverrideDir>,
}

/// Result payload returned from the playback worker thread to the UI thread.
#[derive(Debug, Clone)]
pub struct PlayTrackResult {
    pub generation: u64,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub success: bool,
    pub lyrics_lines: Vec<LyricLine>,
    pub cover_data: Option<CoverArtData>,
    pub presence: PresenceSnapshot,
}

/// Maximum number of concurrent background cover fetch workers.
pub const COVER_FETCH_CONCURRENCY_LIMIT: usize = 4;

/// Commands accepted by the dedicated playback worker thread.
#[derive(Debug)]
pub enum PlaybackWorkerCommand {
    Play(PlayTrackRequest),
    Scrobble { artist: String, title: String, started_unix: u64 },
    FetchCover { url: String, generation: u64 },
    Seek { position_secs: u64 },
}

/// Resolves lyrics from local override files or remote provider off the UI thread.
pub fn resolve_lyrics<L: LyricsSource>(req: &PlayTrackRequest, lyrics: &Arc<L>) -> Vec<LyricLine> {
    if let Some(ref overrides) = req.lyric_overrides {
        if let Some(hit) = overrides.load(&req.track_id) {
            return hit.lines;
        }
    }
    let duration = req.duration_secs.map(u64::from);
    lyrics.lookup(&req.artist, &req.title, duration).map(|s| s.lines).unwrap_or_default()
}

/// Fetches or retrieves cached artwork bytes for a given URL off the UI thread.
fn get_or_fetch_artwork<K: CoverCache>(url: &str, covers: &Arc<Mutex<K>>) -> Option<Vec<u8>> {
    if let Some(data) = covers.lock().ok()?.get(url) {
        debug!(url = %url, bytes = data.len(), "cover cache hit");
        return Some(data);
    }
    let fetched = match fetch_artwork(url, 8) {
        Ok(data) => data,
        Err(err) => {
            warn!(url = %url, error = %err, "failed to fetch cover artwork");
            return None;
        }
    };
    if let Ok(mut cache) = covers.lock() {
        cache.put(url, fetched.clone());
    }
    debug!(url = %url, bytes = fetched.len(), "fetched and cached cover artwork");
    Some(fetched)
}

/// Fetches, caches, and decodes cover artwork for an arbitrary URL off the UI thread.
pub fn fetch_and_decode_cover<K: CoverCache>(
    url: &str,
    covers: &Arc<Mutex<K>>,
    hi_res: bool,
) -> Option<CoverArtData> {
    let bytes = get_or_fetch_artwork(url, covers)?;
    let max_px = if hi_res { COVER_HIRES_MAX_PX } else { COVER_MAX_PX };
    let rgba = match decode_cover(&bytes, max_px) {
        Some(img) => img,
        None => {
            warn!(url = %url, bytes = bytes.len(), "failed to decode cover artwork payload");
            return None;
        }
    };
    Some(CoverArtData::new(url, rgba.as_raw().to_vec(), rgba.width(), rgba.height()))
}

/// Fetches, caches, and decodes cover artwork into raw RGBA bytes off the UI thread.
pub fn resolve_cover_art<K: CoverCache>(
    req: &PlayTrackRequest,
    covers: &Arc<Mutex<K>>,
) -> Option<CoverArtData> {
    let Some(url) = req.artwork_url.as_deref() else {
        debug!(track_id = %req.track_id, "track request has no artwork url; skipping cover resolution");
        return None;
    };
    fetch_and_decode_cover(url, covers, req.hi_res_covers)
}

/// Returns a smaller thumbnail variant URL for row/shelf display if available.
///
/// Keeps high-resolution source for now-playing while reducing row bandwidth:
/// - Replaces `hqdefault` with `mqdefault`
/// - Replaces `-large` suffix with `-small`
/// - Replaces `-thumb-large` with `-thumb-small`
pub fn thumbnail_variant_for_row(url: &str) -> String {
    if url.contains("hqdefault") {
        return url.replace("hqdefault", "mqdefault");
    }
    if let Some(prefix) = url.strip_suffix("-large") {
        return format!("{prefix}-small");
    }
    if url.contains("-thumb-large") {
        return url.replace("-thumb-large", "-thumb-small");
    }
    url.to_string()
}

/// Adapter handles bundled for background playback worker execution.
#[derive(Clone)]
pub struct PlaybackWorkerAdapters<A, L, K> {
    pub audio: Arc<Mutex<A>>,
    pub scrobbler: Arc<Mutex<Box<dyn Scrobbler>>>,
    pub secondary_scrobbler: Option<Arc<Mutex<Box<dyn Scrobbler>>>>,
    pub lyrics: Arc<L>,
    pub covers: Arc<Mutex<K>>,
}

impl<A, L, K> PlaybackWorkerAdapters<A, L, K> {
    pub fn new(
        audio: Arc<Mutex<A>>,
        scrobbler: Arc<Mutex<Box<dyn Scrobbler>>>,
        secondary_scrobbler: Option<Arc<Mutex<Box<dyn Scrobbler>>>>,
        lyrics: Arc<L>,
        covers: Arc<Mutex<K>>,
    ) -> Self {
        Self { audio, scrobbler, secondary_scrobbler, lyrics, covers }
    }
}

/// Executes blocking audio resolve/play, scrobbler, lyrics, and cover fetching on worker thread.
pub fn execute_play_track<A, L, K>(
    req: PlayTrackRequest,
    adapters: &PlaybackWorkerAdapters<A, L, K>,
) -> PlayTrackResult
where
    A: AudioBackend,
    L: LyricsSource,
    K: CoverCache,
{
    let success = match adapters.audio.lock() {
        Ok(mut a) => {
            a.play(&req.track_id);
            a.is_playing() || a.last_requested() == Some(req.track_id.as_str())
        }
        Err(_) => false,
    };

    if let Ok(mut scrob) = adapters.scrobbler.lock() {
        scrob.now_playing(&req.artist, &req.title);
    }
    if let Some(sec) = &adapters.secondary_scrobbler {
        if let Ok(mut scrob) = sec.lock() {
            scrob.now_playing(&req.artist, &req.title);
        }
    }

    let lyrics_lines = resolve_lyrics(&req, &adapters.lyrics);
    let cover_data = resolve_cover_art(&req, &adapters.covers);
    let presence = PresenceSnapshot::new(&req.title, &req.artist, success);

    PlayTrackResult {
        generation: req.generation,
        track_id: req.track_id,
        title: req.title,
        artist: req.artist,
        success,
        lyrics_lines,
        cover_data,
        presence,
    }
}

/// Submits completed scrobble record to primary and optional secondary scrobblers.
pub fn execute_scrobble(
    artist: &str,
    title: &str,
    started_unix: u64,
    scrobbler: &Arc<Mutex<Box<dyn Scrobbler>>>,
    secondary_scrobbler: &Option<Arc<Mutex<Box<dyn Scrobbler>>>>,
) {
    if let Ok(mut scrob) = scrobbler.lock() {
        scrob.scrobble(artist, title, started_unix);
    }
    if let Some(sec) = secondary_scrobbler {
        if let Ok(mut scrob) = sec.lock() {
            scrob.scrobble(artist, title, started_unix);
        }
    }
}

/// Executes audio seek on backend off the UI thread.
pub fn execute_seek<A: AudioBackend>(position_secs: u64, audio: &Arc<Mutex<A>>) {
    if let Ok(mut a) = audio.lock() {
        a.seek(position_secs);
    }
}

/// Executes blocking search request off the UI thread and wraps results in a shelf.
pub fn execute_search<C: Catalog>(
    generation: u64,
    query: &str,
    catalog: &Arc<Mutex<C>>,
    locale: Locale,
) -> (u64, String, Vec<Shelf>) {
    let Some(parsed) = SearchQuery::parse(query) else {
        return (generation, query.to_string(), Vec::new());
    };
    let items = catalog.lock().map(|c| c.search_filtered(&parsed)).unwrap_or_default();
    let mut shelf = Shelf::new(tr(locale, "Search Results"));
    shelf.items = items;
    (generation, query.to_string(), vec![shelf])
}

/// Executes blocking detail lookup off the UI thread.
pub fn execute_detail<C: Catalog>(
    generation: u64,
    browse_id: &str,
    kind: DetailKind,
    catalog: &Arc<Mutex<C>>,
) -> (u64, Option<Detail>) {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return (generation, None);
    }
    let detail = catalog.lock().map(|c| c.detail(trimmed, kind)).unwrap_or(None);
    (generation, detail)
}

/// Refreshes playlists and saved albums from catalog off the UI thread.
pub fn execute_refresh_library<C: Catalog>(
    generation: u64,
    catalog: &Arc<Mutex<C>>,
) -> (u64, Vec<crate::domain::Playlist>, Vec<crate::domain::Album>) {
    let guard = catalog.lock();
    let playlists = guard.as_ref().map(|c| c.playlists()).unwrap_or_default();
    let albums = guard.as_ref().map(|c| c.saved_albums()).unwrap_or_default();
    (generation, playlists, albums)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, StubScrobbler};

    #[test]
    fn test_execute_play_track_in_worker_thread() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        let scrobbler: Arc<Mutex<Box<dyn Scrobbler>>> =
            Arc::new(Mutex::new(Box::new(StubScrobbler::new())));
        let lyrics = Arc::new(LrclibStub::new());
        let covers = Arc::new(Mutex::new(MemoryCoverCache::new()));

        let req = PlayTrackRequest {
            generation: 1,
            track_id: "t1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            artwork_url: None,
            duration_secs: Some(180),
            hi_res_covers: false,
            lyric_overrides: None,
        };

        let adapters = PlaybackWorkerAdapters::new(
            Arc::clone(&audio),
            Arc::clone(&scrobbler),
            None,
            Arc::clone(&lyrics),
            Arc::clone(&covers),
        );

        let handle = std::thread::spawn(move || execute_play_track(req, &adapters));

        let res = handle.join().expect("thread join succeeds");
        assert_eq!(res.generation, 1);
        assert_eq!(res.track_id, "t1");
        assert!(res.success);
        assert_eq!(res.lyrics_lines.len(), 2);
        assert!(res.presence.is_playing);

        assert_eq!(audio.lock().unwrap().last_requested(), Some("t1"));
    }

    #[test]
    fn test_execute_search_in_worker_thread() {
        let catalog = Arc::new(Mutex::new(CatalogStub::new()));
        let cat_clone = Arc::clone(&catalog);

        let handle =
            std::thread::spawn(move || execute_search(42, "rock", &cat_clone, Locale::English));

        let (generation, query, shelves) = handle.join().expect("thread join succeeds");
        assert_eq!(generation, 42);
        assert_eq!(query, "rock");
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].title, "Search Results");

        let handle_es =
            std::thread::spawn(move || execute_search(43, "rock", &catalog, Locale::Spanish));
        let (_, _, shelves_es) = handle_es.join().expect("thread join succeeds");
        assert_eq!(shelves_es[0].title, "Resultados de búsqueda");
    }

    #[test]
    fn test_execute_detail_in_worker_thread() {
        let catalog = Arc::new(Mutex::new(CatalogStub::new()));
        let cat_clone = Arc::clone(&catalog);

        let handle =
            std::thread::spawn(move || execute_detail(7, "album-1", DetailKind::Album, &cat_clone));

        let (generation, detail) = handle.join().expect("thread join succeeds");
        assert_eq!(generation, 7);
        assert!(detail.is_some());
        assert_eq!(detail.unwrap().title, "album-1 Title");
    }

    #[test]
    fn test_execute_scrobble_in_worker_thread() {
        let scrobbler: Arc<Mutex<Box<dyn Scrobbler>>> =
            Arc::new(Mutex::new(Box::new(StubScrobbler::new())));
        let s_clone = Arc::clone(&scrobbler);

        let handle = std::thread::spawn(move || {
            execute_scrobble("Artist", "Song", 123456, &s_clone, &None);
        });

        handle.join().expect("thread join succeeds");
    }

    #[test]
    fn test_worker_channel_dispatch_and_join() {
        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<u64>();

        let handle = std::thread::spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    PlaybackWorkerCommand::Play(req) => {
                        done_tx.send(req.generation).unwrap();
                    }
                    PlaybackWorkerCommand::Scrobble { .. } => {}
                    PlaybackWorkerCommand::FetchCover { generation, .. } => {
                        done_tx.send(generation).unwrap();
                    }
                    PlaybackWorkerCommand::Seek { position_secs } => {
                        done_tx.send(position_secs).unwrap();
                    }
                }
            }
        });

        tx.send(PlaybackWorkerCommand::Play(PlayTrackRequest {
            generation: 99,
            track_id: "test".into(),
            title: "T".into(),
            artist: "A".into(),
            artwork_url: None,
            duration_secs: None,
            hi_res_covers: false,
            lyric_overrides: None,
        }))
        .unwrap();

        let received = done_rx.recv().unwrap();
        assert_eq!(received, 99);

        tx.send(PlaybackWorkerCommand::FetchCover {
            url: "https://artwork.local/art".into(),
            generation: 100,
        })
        .unwrap();
        let cover_gen = done_rx.recv().unwrap();
        assert_eq!(cover_gen, 100);

        tx.send(PlaybackWorkerCommand::Seek { position_secs: 42 }).unwrap();
        let seek_pos = done_rx.recv().unwrap();
        assert_eq!(seek_pos, 42);

        drop(tx);
        handle.join().expect("worker terminates on channel disconnect");
    }

    #[test]
    fn test_execute_seek_in_worker_thread() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        audio.lock().unwrap().play("track-1");
        execute_seek(35, &audio);
        assert_eq!(audio.lock().unwrap().position_secs(), 35);
    }

    #[test]
    fn test_fetch_and_decode_cover_missing_returns_none() {
        let covers = Arc::new(Mutex::new(MemoryCoverCache::new()));
        let res = fetch_and_decode_cover("https://invalid.artwork.local/test", &covers, false);
        assert!(res.is_none());
    }

    #[test]
    fn test_cover_fetch_concurrency_limit_constant() {
        assert_eq!(COVER_FETCH_CONCURRENCY_LIMIT, 4);
    }

    #[test]
    fn test_thumbnail_variant_for_row_replaces_hqdefault_with_mqdefault() {
        let input = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg";
        let expected = "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg";
        assert_eq!(thumbnail_variant_for_row(input), expected);
    }

    #[test]
    fn test_thumbnail_variant_for_row_replaces_large_suffix() {
        let input = "https://lh3.googleusercontent.com/test-thumb-large";
        let expected = "https://lh3.googleusercontent.com/test-thumb-small";
        assert_eq!(thumbnail_variant_for_row(input), expected);

        let input_alt = "https://img.local/cover-large";
        let expected_alt = "https://img.local/cover-small";
        assert_eq!(thumbnail_variant_for_row(input_alt), expected_alt);
    }

    #[test]
    fn test_thumbnail_variant_for_row_leaves_other_urls_untouched() {
        let input = "https://lh3.googleusercontent.com/artist-thumb";
        assert_eq!(thumbnail_variant_for_row(input), input);
    }

    #[test]
    fn test_search_results_sections_resolve_art_from_golden_fixture() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("catalog_search_badges.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: serde_json::Value = serde_json::from_str(&raw).expect("valid fixture json");

        let items = payload
            .pointer(
                "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents",
            )
            .and_then(serde_json::Value::as_array)
            .expect("fixture contents array");

        let mut section_art: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for item in items {
            let Some(renderer) = item.get("musicResponsiveListItemRenderer") else { continue };
            let Some(runs) = renderer
                .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
                .and_then(serde_json::Value::as_array)
            else {
                continue;
            };
            let Some(badge) =
                runs.first().and_then(|r| r.get("text")).and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(thumbs) = renderer
                .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                .and_then(serde_json::Value::as_array)
            else {
                continue;
            };
            let Some(url) =
                thumbs.last().and_then(|t| t.get("url")).and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            section_art.insert(badge.trim().to_ascii_lowercase(), url.trim().to_string());
        }

        // 1. Songs section
        let song_art = section_art.get("song").expect("song section art");
        assert_eq!(song_art, "https://lh3.googleusercontent.com/test-thumb-large");
        assert_eq!(
            thumbnail_variant_for_row(song_art),
            "https://lh3.googleusercontent.com/test-thumb-small"
        );

        // 2. Videos section
        let video_art = section_art.get("video").expect("video section art");
        assert_eq!(video_art, "https://lh3.googleusercontent.com/video-thumb-large");
        assert_eq!(
            thumbnail_variant_for_row(video_art),
            "https://lh3.googleusercontent.com/video-thumb-small"
        );

        // 3. Artists section
        let artist_art = section_art.get("artist").expect("artist section art");
        assert_eq!(artist_art, "https://lh3.googleusercontent.com/artist-thumb");
        assert_eq!(
            thumbnail_variant_for_row(artist_art),
            "https://lh3.googleusercontent.com/artist-thumb"
        );

        // 4. Albums section (represented as Single in badge fixture)
        let album_art = section_art.get("single").expect("album section art");
        assert_eq!(album_art, "https://lh3.googleusercontent.com/single-thumb");
        assert_eq!(
            thumbnail_variant_for_row(album_art),
            "https://lh3.googleusercontent.com/single-thumb"
        );

        // 5. Playlists section
        let playlist_art = section_art.get("playlist").expect("playlist section art");
        assert_eq!(playlist_art, "https://lh3.googleusercontent.com/playlist-thumb");
        assert_eq!(
            thumbnail_variant_for_row(playlist_art),
            "https://lh3.googleusercontent.com/playlist-thumb"
        );
    }
}
