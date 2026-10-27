//! Shared test harness and stubs for AppState unit tests.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::app::AppState;
use crate::domain::{
    Album, Detail, DetailKind, LyricsSnippet, Playlist, SearchQuery, SearchResult, Shelf, Track,
};
use crate::infra::{
    AudioBackend, AudioQuality, AudioStub, Catalog, CatalogStub, LrclibStub, LyricsSource,
    MemoryCoverCache, MemorySessionStore, ReleaseInfo, SyncedLyrics, Updater,
};

#[derive(Default)]
pub(crate) struct CountingCatalog {
    pub(crate) inner: CatalogStub,
    pub(crate) home_calls: AtomicUsize,
    pub(crate) detail_calls: AtomicUsize,
    pub(crate) fail_detail: AtomicBool,
}

impl Catalog for CountingCatalog {
    fn search(&self, query: &str) -> Vec<Track> {
        self.inner.search(query)
    }
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        self.inner.search_filtered(query)
    }
    fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Vec<Track> {
        self.inner.search_by_lyrics(snippet)
    }
    fn radio(&self, seed_track_id: &str) -> Vec<Track> {
        self.inner.radio(seed_track_id)
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        self.home_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.home_shelves()
    }
    fn playlists(&self) -> Vec<Playlist> {
        self.inner.playlists()
    }
    fn liked_tracks(&self) -> Vec<Track> {
        self.inner.liked_tracks()
    }
    fn saved_albums(&self) -> Vec<Album> {
        self.inner.saved_albums()
    }
    fn set_liked(&mut self, track_id: &str, liked: bool) {
        self.inner.set_liked(track_id, liked);
    }
    fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        self.detail_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_detail.load(Ordering::SeqCst) {
            return None;
        }
        self.inner.detail(browse_id, kind)
    }
    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.inner.set_session_cookies(cookies);
    }
}

pub(crate) struct NoUpdates;

impl Updater for NoUpdates {
    fn latest(&self) -> Option<ReleaseInfo> {
        None
    }
}

pub(crate) struct CustomReleaseUpdater(pub(crate) String);

impl Updater for CustomReleaseUpdater {
    fn latest(&self) -> Option<ReleaseInfo> {
        Some(ReleaseInfo { version: self.0.clone(), url: "https://example.com/release".into() })
    }
}

pub(crate) type RecordedLookups = Vec<(String, String, Option<u64>)>;

#[derive(Default, Clone)]
pub(crate) struct TrackingLyricsSource {
    pub(crate) recorded: Arc<Mutex<RecordedLookups>>,
}

impl LyricsSource for TrackingLyricsSource {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        if let Ok(mut lock) = self.recorded.lock() {
            lock.push((artist.to_owned(), title.to_owned(), duration_secs));
        }
        None
    }
}

#[derive(Debug, Default)]
pub(crate) struct QualityTrackingAudio {
    pub(crate) stub: AudioStub,
    pub(crate) applied_quality: AudioQuality,
}

impl AudioBackend for QualityTrackingAudio {
    fn play(&mut self, track_id: &str) {
        self.stub.play(track_id);
    }
    fn pause(&mut self) {
        self.stub.pause();
    }
    fn resume_play(&mut self) {
        self.stub.resume_play();
    }
    fn seek(&mut self, position_secs: u64) {
        self.stub.seek(position_secs);
    }
    fn set_volume(&mut self, volume: u8) {
        self.stub.set_volume(volume);
    }
    fn volume(&self) -> u8 {
        self.stub.volume()
    }
    fn position_secs(&self) -> u64 {
        self.stub.position_secs()
    }
    fn is_playing(&self) -> bool {
        self.stub.is_playing()
    }
    fn last_requested(&self) -> Option<&str> {
        self.stub.last_requested()
    }
    fn set_quality(&mut self, q: AudioQuality) {
        self.applied_quality = q;
    }
}

#[derive(Default)]
pub(crate) struct ImportTestingCatalog {
    pub(crate) inner: CatalogStub,
}

impl Catalog for ImportTestingCatalog {
    fn search(&self, query: &str) -> Vec<Track> {
        self.inner.search(query)
    }
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        self.inner.search_filtered(query)
    }
    fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Vec<Track> {
        self.inner.search_by_lyrics(snippet)
    }
    fn radio(&self, seed_track_id: &str) -> Vec<Track> {
        self.inner.radio(seed_track_id)
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        self.inner.home_shelves()
    }
    fn playlists(&self) -> Vec<Playlist> {
        self.inner.playlists()
    }
    fn liked_tracks(&self) -> Vec<Track> {
        self.inner.liked_tracks()
    }
    fn saved_albums(&self) -> Vec<Album> {
        self.inner.saved_albums()
    }
    fn set_liked(&mut self, track_id: &str, liked: bool) {
        self.inner.set_liked(track_id, liked);
    }
    fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        if browse_id.contains("unknown") {
            return None;
        }
        if browse_id.contains("empty") {
            return Some(Detail::new(kind, "Empty Title", "Subtitle", Vec::new()));
        }
        self.inner.detail(browse_id, kind)
    }
    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.inner.set_session_cookies(cookies);
    }
}

pub(crate) fn wired()
-> AppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache> {
    AppState::new(
        CatalogStub::new(),
        AudioStub::new(),
        MemorySessionStore::new(),
        LrclibStub::new(),
        MemoryCoverCache::new(),
    )
}
