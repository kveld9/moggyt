//! Session state extensions and audio backend synchronization for SlintAppState.

use crate::infra::{decode_handoff, sanitize_cookie_input};

use super::state::SlintAppState;

impl SlintAppState {
    /// Syncs stored session cookies to the audio backend.
    pub fn apply_session_to_audio(&mut self) {
        let cookies = self.session.load();
        if let Ok(mut a) = self.audio.lock() {
            a.set_session_cookies(cookies.as_deref());
        }
    }

    /// Sanitizes cookie input, updates keyring session store, and binds catalog and audio backends.
    pub fn sign_in_from_input(&mut self, cleaned: &str) -> bool {
        let Some(cookies) = sanitize_cookie_input(cleaned) else {
            return false;
        };
        self.session.save(&cookies);
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(Some(&cookies));
        }
        self.apply_session_to_audio();
        self.signed_in = true;
        true
    }

    /// Imports session from companion QR/handoff code string.
    pub fn import_handoff(&mut self, text: &str) -> bool {
        let Ok(cookies) = decode_handoff(text) else {
            return false;
        };
        self.session.save(&cookies);
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(Some(&cookies));
        }
        self.apply_session_to_audio();
        self.signed_in = true;
        true
    }

    /// Clears active session and resets catalog and audio backends to anonymous state.
    pub fn sign_out(&mut self) {
        self.session.clear();
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(None);
        }
        self.apply_session_to_audio();
        self.signed_in = false;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::infra::{AudioBackend, AudioError, AudioStub, MemorySessionStore, SessionStore};
    use crate::slint_shell::test_support::test_wired;

    struct SessionTrackingAudio {
        stub: AudioStub,
        cookies: Arc<Mutex<Option<String>>>,
    }

    impl AudioBackend for SessionTrackingAudio {
        fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
            self.stub.play(track_id)
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
        fn set_session_cookies(&mut self, cookies: Option<&str>) {
            *self.cookies.lock().unwrap() = cookies.map(ToString::to_string);
        }
    }

    #[test]
    fn test_sign_in_and_out_lifecycle_syncs_session() {
        let mut state = test_wired();
        assert!(!state.signed_in);
        assert!(!state.sign_in_from_input("   "));
        assert!(state.sign_in_from_input("SID=valid_cookie_123; SAPISID=valid_sapisid_123"));
        assert!(state.signed_in);
        assert_eq!(
            state.session.load().as_deref(),
            Some("SID=valid_cookie_123; SAPISID=valid_sapisid_123")
        );
        state.sign_out();
        assert!(!state.signed_in);
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn test_import_handoff_syncs_session() {
        let mut state = test_wired();
        assert!(!state.signed_in);
        assert!(!state.import_handoff("invalid_handoff"));

        let raw = "SID=handoff_cookie; SAPISID=handoff_sapisid";
        let envelope = crate::infra::encode_handoff(raw).unwrap();
        assert!(state.import_handoff(&envelope));
        assert!(state.signed_in);
        assert_eq!(state.session.load().as_deref(), Some(raw));
    }

    #[test]
    fn test_apply_session_to_audio_wiring_lifecycle() {
        let mut session = MemorySessionStore::new();
        session.save("SAPISID=startup_cookie");
        let catalog = crate::infra::CatalogStub::new();
        let audio_cookies = Arc::new(Mutex::new(None));
        let audio =
            SessionTrackingAudio { stub: AudioStub::new(), cookies: Arc::clone(&audio_cookies) };
        let lyrics = crate::infra::LrclibStub::new();
        let covers = crate::infra::MemoryCoverCache::new();

        let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);
        assert_eq!(audio_cookies.lock().unwrap().as_deref(), Some("SAPISID=startup_cookie"));

        state.sign_out();
        assert_eq!(*audio_cookies.lock().unwrap(), None);

        assert!(state.sign_in_from_input("SAPISID=runtime_cookie"));
        assert_eq!(audio_cookies.lock().unwrap().as_deref(), Some("SAPISID=runtime_cookie"));
    }
}
