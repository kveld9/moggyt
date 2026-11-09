#![cfg(test)]
//! Shared test support fakes and state fixtures for UI shell tests.
//!
//! Hexagonal invariant: in-memory stubs only; zero network, zero audio hardware, zero async.

use std::sync::{Arc, Mutex};

use crate::domain::{Album, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track};
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioError, AudioStub, Catalog, CatalogError, CatalogStub, LrclibStub,
    MemoryCoverCache, MemorySessionStore,
};
use crate::slint_shell::state::SlintAppState;

/// Constructs a SlintAppState wired with memory-only test stub adapters.
pub fn test_wired() -> SlintAppState {
    let catalog = CatalogStub::new();
    let audio = AudioStub::new();
    let session = MemorySessionStore::new();
    let lyrics = LrclibStub::new();
    let covers = MemoryCoverCache::new();
    SlintAppState::new(catalog, audio, session, lyrics, covers)
}

pub type LocaleLog = Arc<Mutex<Vec<Locale>>>;

/// Mock catalog recording locale setting and home shelf requests for verification.
#[derive(Debug, Clone)]
pub struct LocaleTrackingCatalog {
    pub locales: LocaleLog,
    pub fetch_locales: LocaleLog,
    pub current: Arc<Mutex<Locale>>,
}

impl LocaleTrackingCatalog {
    pub fn new() -> (Self, LocaleLog, LocaleLog) {
        let locales = Arc::new(Mutex::new(Vec::new()));
        let fetch_locales = Arc::new(Mutex::new(Vec::new()));
        let current = Arc::new(Mutex::new(Locale::default()));
        (
            Self {
                locales: Arc::clone(&locales),
                fetch_locales: Arc::clone(&fetch_locales),
                current,
            },
            locales,
            fetch_locales,
        )
    }
}

impl Catalog for LocaleTrackingCatalog {
    fn search(&self, _query: &str) -> Result<Vec<Track>, CatalogError> {
        Ok(Vec::new())
    }

    fn search_filtered(&self, _query: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        Ok(Vec::new())
    }

    fn radio(&self, _track_id: &str) -> Vec<Track> {
        Vec::new()
    }

    fn home_shelves(&self) -> Vec<Shelf> {
        let loc = *self.current.lock().unwrap();
        self.fetch_locales.lock().unwrap().push(loc);
        let title = match loc {
            Locale::English => "Home English",
            Locale::Spanish => "Home Spanish",
        };
        vec![Shelf::new(title)]
    }

    fn playlists(&self) -> Result<Vec<Playlist>, CatalogError> {
        Ok(Vec::new())
    }

    fn saved_albums(&self) -> Result<Vec<Album>, CatalogError> {
        Ok(Vec::new())
    }

    fn detail(&self, _browse_id: &str, _kind: DetailKind) -> Result<Option<Detail>, CatalogError> {
        Ok(None)
    }

    fn set_locale(&mut self, locale: Locale) {
        self.locales.lock().unwrap().push(locale);
        *self.current.lock().unwrap() = locale;
    }
}

/// Mock catalog returning configurable library results for testing.
#[derive(Debug, Clone)]
pub struct MockLibraryCatalog {
    pub playlists_res: Result<Vec<Playlist>, CatalogError>,
    pub albums_res: Result<Vec<Album>, CatalogError>,
    pub search_res: Result<Vec<Track>, CatalogError>,
    pub search_filtered_res: Result<Vec<SearchResult>, CatalogError>,
    pub detail_res: Result<Option<Detail>, CatalogError>,
    pub radio_res: Vec<Track>,
}

impl Default for MockLibraryCatalog {
    fn default() -> Self {
        Self {
            playlists_res: Ok(Vec::new()),
            albums_res: Ok(Vec::new()),
            search_res: Ok(Vec::new()),
            search_filtered_res: Ok(Vec::new()),
            detail_res: Ok(None),
            radio_res: Vec::new(),
        }
    }
}

impl MockLibraryCatalog {
    pub fn new(
        playlists: Result<Vec<Playlist>, CatalogError>,
        albums: Result<Vec<Album>, CatalogError>,
    ) -> Self {
        Self {
            playlists_res: playlists,
            albums_res: albums,
            search_res: Ok(Vec::new()),
            search_filtered_res: Ok(Vec::new()),
            detail_res: Ok(None),
            radio_res: Vec::new(),
        }
    }

    pub fn with_search(mut self, res: Result<Vec<Track>, CatalogError>) -> Self {
        self.search_res = res;
        self
    }

    pub fn with_search_filtered(mut self, res: Result<Vec<SearchResult>, CatalogError>) -> Self {
        self.search_filtered_res = res;
        self
    }

    pub fn with_detail(mut self, res: Result<Option<Detail>, CatalogError>) -> Self {
        self.detail_res = res;
        self
    }

    pub fn with_radio(mut self, tracks: Vec<Track>) -> Self {
        self.radio_res = tracks;
        self
    }
}

impl Catalog for MockLibraryCatalog {
    fn search(&self, _: &str) -> Result<Vec<Track>, CatalogError> {
        self.search_res.clone()
    }
    fn search_filtered(&self, _: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        self.search_filtered_res.clone()
    }
    fn radio(&self, _: &str) -> Vec<Track> {
        self.radio_res.clone()
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        Vec::new()
    }
    fn playlists(&self) -> Result<Vec<Playlist>, CatalogError> {
        self.playlists_res.clone()
    }
    fn saved_albums(&self) -> Result<Vec<Album>, CatalogError> {
        self.albums_res.clone()
    }
    fn detail(&self, _: &str, _: DetailKind) -> Result<Option<Detail>, CatalogError> {
        self.detail_res.clone()
    }
}

/// Fake audio backend representing a track that reached its end with a drained sink.
#[derive(Debug)]
pub struct EndedAudio {
    track_id: Option<String>,
    duration_secs: u64,
    position_secs: u64,
    playing: bool,
    paused: bool,
    empty: bool,
    volume: u8,
}

impl EndedAudio {
    pub fn new(track_id: &str, duration_secs: u64) -> Self {
        Self {
            track_id: Some(track_id.to_owned()),
            duration_secs,
            position_secs: duration_secs,
            playing: false,
            paused: false,
            empty: true,
            volume: 100,
        }
    }
}

impl AudioBackend for EndedAudio {
    fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
        self.track_id = Some(track_id.to_owned());
        self.playing = true;
        self.paused = false;
        self.empty = false;
        self.position_secs = 0;
        Ok(())
    }

    fn pause(&mut self) {
        self.playing = false;
        self.paused = true;
    }

    fn resume_play(&mut self) {
        if self.track_id.is_some() {
            self.playing = true;
            self.paused = false;
            self.empty = false;
        }
    }

    fn seek(&mut self, position_secs: u64) {
        self.position_secs = position_secs;
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn position_secs(&self) -> u64 {
        self.position_secs
    }

    fn is_playing(&self) -> bool {
        self.playing
    }

    fn is_paused(&self) -> bool {
        self.paused
    }

    fn is_empty(&self) -> bool {
        self.empty
    }

    fn duration_secs(&self) -> u64 {
        self.duration_secs
    }

    fn last_requested(&self) -> Option<&str> {
        self.track_id.as_deref()
    }
}

/// Fake audio backend returning Ok on play but leaving last_requested empty.
#[derive(Debug, Default)]
pub struct NoLastRequestedAudio;

impl AudioBackend for NoLastRequestedAudio {
    fn play(&mut self, _track_id: &str) -> Result<(), AudioError> {
        Ok(())
    }

    fn pause(&mut self) {}

    fn resume_play(&mut self) {}

    fn seek(&mut self, _position_secs: u64) {}

    fn set_volume(&mut self, _volume: u8) {}

    fn volume(&self) -> u8 {
        100
    }

    fn position_secs(&self) -> u64 {
        0
    }

    fn is_playing(&self) -> bool {
        true
    }

    fn last_requested(&self) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::page::SlintPage;

    #[test]
    fn test_test_wired_builds_default_search_page() {
        let state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        assert!(state.queue.is_empty());
        assert_eq!(state.locale, Locale::default());
    }

    #[test]
    fn test_ended_audio_defaults_and_transitions() {
        let mut audio = EndedAudio::new("trk-1", 180);
        assert_eq!(audio.last_requested(), Some("trk-1"));
        assert!(!audio.is_playing());
        assert!(!audio.is_paused());
        assert!(audio.is_empty());
        assert_eq!(audio.position_secs(), 180);
        assert_eq!(audio.duration_secs(), 180);
        assert_eq!(audio.volume(), 100);

        audio.seek(42);
        assert_eq!(audio.position_secs(), 42);

        let _ = audio.play("trk-2");
        assert_eq!(audio.last_requested(), Some("trk-2"));
        assert!(audio.is_playing());
        assert!(!audio.is_empty());
        assert_eq!(audio.position_secs(), 0);

        audio.pause();
        assert!(!audio.is_playing());
        assert!(audio.is_paused());

        audio.resume_play();
        assert!(audio.is_playing());
        assert!(!audio.is_paused());
        assert!(!audio.is_empty());
    }
}
