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

use crate::domain::{Detail, DetailKind, PlaybackFailure};
#[cfg(test)]
use crate::domain::{LyricsSnippet, SearchQuery, SearchResult, Shelf};
#[cfg(test)]
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioError, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog, CatalogError, CoverCache,
    LyricLine, LyricOverrideDir, LyricsSource, Scrobbler, decode_cover, fetch_artwork,
};
use crate::slint_shell::library_fetch::LibraryFetch;
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
    /// True when this request preloads the next track behind the active sink.
    pub gapless: bool,
}

/// Result payload returned from the playback worker thread to the UI thread.
#[derive(Debug, Clone)]
pub struct PlayTrackResult {
    pub generation: u64,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub success: bool,
    /// Classified failure when `success` is false.
    pub failure: Option<PlaybackFailure>,
    pub lyrics_lines: Vec<LyricLine>,
    pub cover_data: Option<CoverArtData>,
    pub presence: PresenceSnapshot,
    /// True when this result belongs to a gapless preload.
    pub gapless: bool,
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
    req: &PlayTrackRequest,
) -> Result<(), AudioError> {
    if req.gapless {
        return audio.play_gapless(&req.track_id);
    }
    if audio.is_playing() && req.crossfade_seconds > 0 {
        return audio.play_crossfade(
            &req.track_id,
            std::time::Duration::from_secs(u64::from(req.crossfade_seconds)),
        );
    }
    audio.play(&req.track_id)
}

/// Runs the backend call and reports audibility plus the classified failure.
fn evaluate_play_outcome<A: AudioBackend>(
    audio: &mut A,
    req: &PlayTrackRequest,
) -> (bool, Option<PlaybackFailure>) {
    let started = dispatch_backend_play(audio, req);
    let requested = audio.last_requested() == Some(req.track_id.as_str());
    let audible = req.gapless || audio.is_playing();
    if let Err(error) = started {
        return (false, Some(error.failure()));
    }
    if requested && audible {
        return (true, None);
    }
    (false, Some(PlaybackFailure::Unknown))
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
    let (success, failure) = match adapters.audio.lock() {
        Ok(mut a) => evaluate_play_outcome(&mut *a, &req),
        Err(_) => (false, Some(PlaybackFailure::Unknown)),
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
        failure,
        lyrics_lines,
        cover_data,
        presence,
        gapless: req.gapless,
    }
}

/// Executes audio seek on backend off the UI thread.
pub fn execute_seek<A: AudioBackend>(position_secs: u64, audio: &Arc<Mutex<A>>) {
    if let Ok(mut a) = audio.lock() {
        a.seek(position_secs);
    }
}

pub use super::worker_catalog::{execute_search, fetch_detail, resolve_search};

/// Executes blocking detail lookup off the UI thread.
pub fn execute_detail<C: Catalog>(
    generation: u64,
    browse_id: &str,
    kind: DetailKind,
    catalog: &Arc<Mutex<C>>,
) -> (u64, Result<Option<Detail>, CatalogError>) {
    (generation, fetch_detail(browse_id, kind, catalog))
}

/// Refreshes every library section from catalog off the UI thread.
pub fn execute_refresh_library<C: Catalog>(
    generation: u64,
    catalog: &Arc<Mutex<C>>,
) -> (u64, LibraryFetch) {
    let guard = catalog.lock();
    let fetch = guard
        .as_ref()
        .map(|c| LibraryFetch {
            playlists: c.playlists(),
            songs: c.library_songs(),
            albums: c.saved_albums(),
            artists: c.library_artists(),
            podcasts: c.library_podcasts(),
            profiles: c.library_profiles(),
        })
        .unwrap_or(LibraryFetch {
            playlists: Err(CatalogError::Transport),
            songs: Err(CatalogError::Transport),
            albums: Err(CatalogError::Transport),
            artists: Err(CatalogError::Transport),
            podcasts: Err(CatalogError::Transport),
            profiles: Err(CatalogError::Transport),
        });
    (generation, fetch)
}

#[cfg(test)]
#[path = "playback_worker_tests.rs"]
mod tests;
