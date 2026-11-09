#![cfg(test)]
//! Shared test support fakes and state fixtures for UI shell tests.
//!
//! Hexagonal invariant: in-memory stubs only; zero network, zero audio hardware, zero async.

use std::sync::{Arc, Mutex};

use crate::domain::{Album, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track};
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioStub, Catalog, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore,
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
    fn search(&self, _query: &str) -> Vec<Track> {
        Vec::new()
    }

    fn search_filtered(&self, _query: &SearchQuery) -> Vec<SearchResult> {
        Vec::new()
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

    fn playlists(&self) -> Vec<Playlist> {
        Vec::new()
    }

    fn saved_albums(&self) -> Vec<Album> {
        Vec::new()
    }

    fn detail(&self, _browse_id: &str, _kind: DetailKind) -> Option<Detail> {
        None
    }

    fn set_locale(&mut self, locale: Locale) {
        self.locales.lock().unwrap().push(locale);
        *self.current.lock().unwrap() = locale;
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
    fn play(&mut self, track_id: &str) {
        self.track_id = Some(track_id.to_owned());
        self.playing = true;
        self.paused = false;
        self.empty = false;
        self.position_secs = 0;
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

        audio.play("trk-2");
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
