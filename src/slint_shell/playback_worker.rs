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

use crate::domain::{Detail, DetailKind};
#[cfg(test)]
use crate::domain::{LyricsSnippet, SearchQuery, SearchResult, Shelf};
#[cfg(test)]
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioError, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog, CatalogError, CoverCache,
    LyricLine, LyricOverrideDir, LyricsSource, Scrobbler, decode_cover, fetch_artwork,
};
use crate::slint_shell::platform::{CoverArtData, PresenceSnapshot};

/// Target track metadata for background lyrics prefetching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsPrefetch {
    pub artist: String,
    pub title: String,
    pub duration_secs: Option<u32>,
}

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
    /// Crossfade transition duration in seconds (0 = disabled).
    pub crossfade_seconds: u32,
    pub lyric_overrides: Option<LyricOverrideDir>,
    pub prefetch_lyrics: Option<LyricsPrefetch>,
    /// Target video ID of the upcoming track for background stream URL prefetching.
    pub prefetch_stream_id: Option<String>,
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

/// Spawns a background thread to prefetch lyrics for the next track into cache.
///
/// Returns `None` without spawning if `prefetch` is `None` or if either artist or title
/// is blank whitespace. The spawned thread runs as `"moggyt-lyrics-prefetch"`.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn spawn_lyrics_prefetch<L: LyricsSource + Send + Sync + ?Sized + 'static>(
    prefetch: Option<LyricsPrefetch>,
    lyrics: Arc<L>,
) -> Option<std::thread::JoinHandle<()>> {
    let prefetch = prefetch?;
    let trimmed_artist = prefetch.artist.trim();
    let trimmed_title = prefetch.title.trim();
    if trimmed_artist.is_empty() || trimmed_title.is_empty() {
        return None;
    }
    let artist = trimmed_artist.to_string();
    let title = trimmed_title.to_string();
    let duration = prefetch.duration_secs.map(u64::from);

    match std::thread::Builder::new().name("moggyt-lyrics-prefetch".into()).spawn(move || {
        let _ = lyrics.lookup(&artist, &title, duration);
    }) {
        Ok(handle) => Some(handle),
        Err(err) => {
            warn!(error = %err, "failed to spawn lyrics prefetch thread");
            None
        }
    }
}

/// Fetches or retrieves cached artwork bytes for a given URL off the UI thread.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
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
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
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
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
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

fn dispatch_backend_play<A: AudioBackend>(
    audio: &mut A,
    track_id: &str,
    crossfade_seconds: u32,
) -> Result<(), AudioError> {
    if audio.is_playing() && crossfade_seconds > 0 {
        audio.play_crossfade(track_id, std::time::Duration::from_secs(u64::from(crossfade_seconds)))
    } else {
        audio.play(track_id)
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
            let started =
                dispatch_backend_play(&mut *a, &req.track_id, req.crossfade_seconds).is_ok();
            started && a.last_requested() == Some(req.track_id.as_str()) && a.is_playing()
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

/// Executes audio seek on backend off the UI thread.
pub fn execute_seek<A: AudioBackend>(position_secs: u64, audio: &Arc<Mutex<A>>) {
    if let Ok(mut a) = audio.lock() {
        a.seek(position_secs);
    }
}

pub use super::worker_catalog::{execute_search, resolve_search};

/// Executes blocking detail lookup off the UI thread.
pub fn execute_detail<C: Catalog>(
    generation: u64,
    browse_id: &str,
    kind: DetailKind,
    catalog: &Arc<Mutex<C>>,
) -> (u64, Result<Option<Detail>, CatalogError>) {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return (generation, Ok(None));
    }
    let res =
        catalog.lock().map_err(|_| CatalogError::Transport).and_then(|c| c.detail(trimmed, kind));
    (generation, res)
}

type PlaylistResult = Result<Vec<crate::domain::Playlist>, CatalogError>;
type AlbumResult = Result<Vec<crate::domain::Album>, CatalogError>;

/// Refreshes playlists and saved albums from catalog off the UI thread.
pub fn execute_refresh_library<C: Catalog>(
    generation: u64,
    catalog: &Arc<Mutex<C>>,
) -> (u64, PlaylistResult, AlbumResult) {
    let guard = catalog.lock();
    let playlists = guard.as_ref().map(|c| c.playlists()).unwrap_or(Err(CatalogError::Transport));
    let albums = guard.as_ref().map(|c| c.saved_albums()).unwrap_or(Err(CatalogError::Transport));
    (generation, playlists, albums)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MusicVideoType, SearchFilter, Track};
    use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, StubScrobbler};
    use crate::slint_shell::test_support::NoLastRequestedAudio;

    fn dummy_play_req(track_id: &str) -> PlayTrackRequest {
        PlayTrackRequest {
            generation: 1,
            track_id: track_id.into(),
            title: "Song".into(),
            artist: "Artist".into(),
            artwork_url: None,
            duration_secs: Some(180),
            hi_res_covers: false,
            crossfade_seconds: 0,
            lyric_overrides: None,
            prefetch_lyrics: None,
            prefetch_stream_id: None,
        }
    }

    fn test_worker_adapters<A: AudioBackend>(
        audio: Arc<Mutex<A>>,
    ) -> PlaybackWorkerAdapters<A, LrclibStub, MemoryCoverCache> {
        let scrobbler = Arc::new(Mutex::new(Box::new(StubScrobbler::new()) as Box<dyn Scrobbler>));
        let lyrics = Arc::new(LrclibStub::new());
        let covers = Arc::new(Mutex::new(MemoryCoverCache::new()));
        PlaybackWorkerAdapters::new(audio, scrobbler, None, lyrics, covers)
    }

    #[test]
    fn test_execute_play_track_in_worker_thread() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        let adapters = test_worker_adapters(Arc::clone(&audio));
        let req = dummy_play_req("t1");

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
    fn failed_play_while_other_track_plays_reports_failure() {
        let mut stub = AudioStub::new();
        assert_eq!(stub.play("track-A"), Ok(()));
        stub.set_fail_plays(true);
        let audio = Arc::new(Mutex::new(stub));
        let adapters = test_worker_adapters(Arc::clone(&audio));
        let res = execute_play_track(dummy_play_req("track-B"), &adapters);
        assert!(!res.success);
        let guard = audio.lock().unwrap();
        assert_eq!(guard.last_requested(), Some("track-A"));
        assert!(guard.is_playing());
    }

    #[test]
    fn success_requires_requested_id_match() {
        let audio = Arc::new(Mutex::new(NoLastRequestedAudio));
        let adapters = test_worker_adapters(audio);
        let res = execute_play_track(dummy_play_req("track-C"), &adapters);
        assert!(!res.success);
    }

    #[test]
    fn test_execute_search_in_worker_thread() {
        let catalog = Arc::new(Mutex::new(CatalogStub::new()));
        let (cat_clone, q) =
            (Arc::clone(&catalog), SearchQuery::parse("rock").expect("valid query"));
        let q_clone = q.clone();
        let handle = std::thread::spawn(move || {
            execute_search(42, &q_clone, &cat_clone, Locale::English, false)
        });

        let (generation, query, shelves) = handle.join().expect("thread join succeeds");
        let shelves = shelves.expect("shelves ok");
        assert_eq!(generation, 42);
        assert_eq!(query, "rock");
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].title, "Search Results");

        let cat_es = Arc::clone(&catalog);
        let handle_es =
            std::thread::spawn(move || execute_search(43, &q, &cat_es, Locale::Spanish, false));
        let (_, _, shelves_es) = handle_es.join().expect("thread join succeeds");
        let shelves_es = shelves_es.expect("shelves_es ok");
        assert_eq!(shelves_es[0].title, "Resultados de búsqueda");

        let q_albums =
            SearchQuery::parse("rock").expect("valid query").with_filter(SearchFilter::Albums);
        let (_, _, shelves_albums) =
            execute_search(44, &q_albums, &catalog, Locale::English, false);
        let shelves_albums = shelves_albums.expect("shelves_albums ok");
        assert_eq!(shelves_albums[0].items.len(), 1);
        assert_eq!(shelves_albums[0].items[0].kind(), SearchFilter::Albums);
    }

    #[derive(Debug, Default)]
    struct VideoTrackSearchCatalog;

    impl Catalog for VideoTrackSearchCatalog {
        fn search(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
            Ok(Vec::new())
        }
        fn search_filtered(&self, _: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
            let t1 = Track::new("t1", "Video", "Artist").with_video_type(MusicVideoType::Omv);
            let t2 = Track::new("t2", "Song", "Artist").with_video_type(MusicVideoType::Atv);
            Ok(vec![SearchResult::Track(t1), SearchResult::Track(t2)])
        }
        fn search_by_lyrics(&self, s: &LyricsSnippet) -> Result<Vec<Track>, CatalogError> {
            Ok(vec![Track::new("lyric-hit", s.as_str(), "Artist")])
        }
        fn radio(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
            Ok(Vec::new())
        }
        fn home_shelves(&self) -> Vec<Shelf> {
            Vec::new()
        }
        fn playlists(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
            Ok(Vec::new())
        }
        fn saved_albums(&self) -> Result<Vec<crate::domain::Album>, CatalogError> {
            Ok(Vec::new())
        }
        fn detail(&self, _: &str, _: DetailKind) -> Result<Option<Detail>, CatalogError> {
            Ok(None)
        }
    }

    #[test]
    fn test_execute_search_routes_by_lyrics_flag() {
        let catalog = Arc::new(Mutex::new(VideoTrackSearchCatalog));
        let q = SearchQuery::parse("text").expect("query");
        let (_, _, plain) = execute_search(1, &q, &catalog, Locale::English, false);
        assert_eq!(plain.expect("plain")[0].items.len(), 2);

        let lyrics = q.with_lyrics(true);
        let (_, _, lyric_res) = execute_search(2, &lyrics, &catalog, Locale::English, false);
        let items = &lyric_res.expect("lyrics")[0].items;
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], crate::domain::SearchResult::Track(t) if t.id == "lyric-hit"));
    }

    #[test]
    fn test_execute_search_with_songs_only_drops_video() {
        let catalog = Arc::new(Mutex::new(VideoTrackSearchCatalog));
        let q = SearchQuery::parse("query").expect("valid query");
        let (_, _, shelves) = execute_search(1, &q, &catalog, Locale::English, true);
        let shelves = shelves.expect("shelves ok");
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].items.len(), 1);
        match &shelves[0].items[0] {
            crate::domain::SearchResult::Track(t) => assert_eq!(t.id, "t2"),
            _ => panic!("expected track result"),
        }
    }

    struct BadgeVideoSearchCatalog;

    impl Catalog for BadgeVideoSearchCatalog {
        fn search(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
            Ok(Vec::new())
        }
        fn search_filtered(
            &self,
            _: &SearchQuery,
        ) -> Result<Vec<crate::domain::SearchResult>, CatalogError> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/catalog_search_badges.json");
            let raw = std::fs::read_to_string(path).expect("fixture readable");
            let payload: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
            let hits = crate::infra::catalog_parse_search::parse_search_all(&payload);
            Ok(crate::infra::LiveCatalog::map_hits(hits, crate::domain::SearchFilter::All))
        }
        fn radio(&self, _: &str) -> Result<Vec<crate::domain::Track>, CatalogError> {
            Ok(Vec::new())
        }
        fn home_shelves(&self) -> Vec<Shelf> {
            Vec::new()
        }
        fn playlists(&self) -> Result<Vec<crate::domain::Playlist>, CatalogError> {
            Ok(Vec::new())
        }
        fn saved_albums(&self) -> Result<Vec<crate::domain::Album>, CatalogError> {
            Ok(Vec::new())
        }
        fn detail(&self, _: &str, _: DetailKind) -> Result<Option<Detail>, CatalogError> {
            Ok(None)
        }
    }

    #[test]
    fn test_execute_search_with_songs_only_drops_video_classified_by_badge() {
        let catalog = Arc::new(Mutex::new(BadgeVideoSearchCatalog));
        let q = SearchQuery::parse("bachata").expect("valid query");
        let (_, _, unfiltered) = execute_search(1, &q, &catalog, Locale::English, false);
        let (_, _, filtered) = execute_search(1, &q, &catalog, Locale::English, true);
        let unfiltered = unfiltered.expect("unfiltered ok");
        let filtered = filtered.expect("filtered ok");

        let contains_video = |shelves: &[Shelf]| {
            shelves.iter().flat_map(|s| &s.items).any(|item| match item {
                crate::domain::SearchResult::Track(t) => t.id == "video-vid-2",
                _ => false,
            })
        };

        assert!(contains_video(&unfiltered), "unfiltered search includes badge-classified video");
        assert!(!contains_video(&filtered), "songs-only search drops badge-classified video");
    }

    #[test]
    fn test_execute_detail_in_worker_thread() {
        let catalog = Arc::new(Mutex::new(CatalogStub::new()));
        let cat_clone = Arc::clone(&catalog);

        let handle =
            std::thread::spawn(move || execute_detail(7, "album-1", DetailKind::Album, &cat_clone));

        let (generation, detail) = handle.join().expect("thread join succeeds");
        let detail = detail.expect("detail ok");
        assert_eq!(generation, 7);
        assert!(detail.is_some());
        assert_eq!(detail.unwrap().title, "album-1 Title");
    }

    #[test]
    fn test_worker_channel_dispatch_and_join() {
        let (tx, rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<u64>();

        let handle = std::thread::spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    PlaybackWorkerCommand::Play(req) => done_tx.send(req.generation).unwrap(),
                    PlaybackWorkerCommand::FetchCover { generation, .. } => {
                        done_tx.send(generation).unwrap()
                    }
                    PlaybackWorkerCommand::Seek { position_secs } => {
                        done_tx.send(position_secs).unwrap()
                    }
                }
            }
        });

        let mut play_req = dummy_play_req("test");
        play_req.generation = 99;
        tx.send(PlaybackWorkerCommand::Play(play_req)).unwrap();
        assert_eq!(done_rx.recv().unwrap(), 99);

        tx.send(PlaybackWorkerCommand::FetchCover {
            url: "https://artwork.local/art".into(),
            generation: 100,
        })
        .unwrap();
        assert_eq!(done_rx.recv().unwrap(), 100);

        tx.send(PlaybackWorkerCommand::Seek { position_secs: 42 }).unwrap();
        assert_eq!(done_rx.recv().unwrap(), 42);

        drop(tx);
        handle.join().expect("worker terminates on channel disconnect");
    }

    #[test]
    fn test_execute_seek_in_worker_thread() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        let _ = audio.lock().unwrap().play("track-1");
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
        assert_eq!(
            thumbnail_variant_for_row("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"),
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        );
    }

    #[test]
    fn test_thumbnail_variant_for_row_replaces_large_suffix() {
        assert_eq!(
            thumbnail_variant_for_row("https://lh3.googleusercontent.com/test-thumb-large"),
            "https://lh3.googleusercontent.com/test-thumb-small"
        );
        assert_eq!(
            thumbnail_variant_for_row("https://img.local/cover-large"),
            "https://img.local/cover-small"
        );
    }

    #[test]
    fn test_thumbnail_variant_for_row_leaves_other_urls_untouched() {
        assert_eq!(
            thumbnail_variant_for_row("https://lh3.googleusercontent.com/artist-thumb"),
            "https://lh3.googleusercontent.com/artist-thumb"
        );
    }

    #[test]
    fn test_search_results_sections_resolve_art_from_golden_fixture() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog_search_badges.json");
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

        const B: &str = "https://lh3.googleusercontent.com";
        let cases = [
            ("song", format!("{B}/test-thumb-large"), format!("{B}/test-thumb-small")),
            ("video", format!("{B}/video-thumb-large"), format!("{B}/video-thumb-small")),
            ("artist", format!("{B}/artist-thumb"), format!("{B}/artist-thumb")),
            ("single", format!("{B}/single-thumb"), format!("{B}/single-thumb")),
            ("playlist", format!("{B}/playlist-thumb"), format!("{B}/playlist-thumb")),
        ];
        for (section, raw, small) in cases {
            let art = section_art.get(section).expect("section art");
            assert_eq!(art, &raw);
            assert_eq!(thumbnail_variant_for_row(art), small);
        }
    }

    #[test]
    fn test_spawn_lyrics_prefetch_blank_or_none_returns_none() {
        let lyrics = Arc::new(LrclibStub::new());
        assert!(spawn_lyrics_prefetch(None, Arc::clone(&lyrics)).is_none());

        let blanks = [
            ("", "Title"),
            ("   ", "Title"),
            ("Artist", ""),
            ("Artist", "   "),
            ("", ""),
            ("  ", "  "),
        ];
        for (artist, title) in blanks {
            let prefetch = LyricsPrefetch {
                artist: artist.into(),
                title: title.into(),
                duration_secs: Some(120),
            };
            assert!(spawn_lyrics_prefetch(Some(prefetch), Arc::clone(&lyrics)).is_none());
        }
    }

    struct CountingPrefetchLyrics {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl LyricsSource for CountingPrefetchLyrics {
        fn lookup(
            &self,
            artist: &str,
            title: &str,
            _duration_secs: Option<u64>,
        ) -> Option<crate::infra::SyncedLyrics> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(crate::infra::SyncedLyrics {
                lines: vec![LyricLine::new(0, format!("{title} by {artist}"))],
            })
        }
    }

    #[test]
    fn test_spawn_lyrics_prefetch_warms_cache() {
        let fake = CountingPrefetchLyrics { calls: std::sync::atomic::AtomicUsize::new(0) };
        let cached = Arc::new(crate::infra::CachedLyrics::new(fake));

        let prefetch = LyricsPrefetch {
            artist: "Radiohead".into(),
            title: "Creep".into(),
            duration_secs: Some(236),
        };

        let handle = spawn_lyrics_prefetch(Some(prefetch), Arc::clone(&cached));
        let handle = handle.expect("prefetch thread spawned");
        handle.join().expect("prefetch thread join succeeds");

        assert_eq!(cached.inner().calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let hit = cached.lookup("Radiohead", "Creep", Some(236));
        assert!(hit.is_some());
        assert_eq!(cached.inner().calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
