//! User authentication session, cookies, and handoff import for [`AppState`].

use crate::app::AppState;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore, decode_handoff,
    sanitize_cookie_input,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Returns true if an active session is currently stored.
    pub fn is_signed_in(&self) -> bool {
        self.signed_in
    }

    fn apply_session(&mut self, cleaned: &str) {
        self.session.save(cleaned);
        self.catalog.set_session_cookies(Some(cleaned));
        self.signed_in = true;
    }

    /// Attempts to parse and save the current cookie input.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn sign_in_from_input(&mut self) -> bool {
        let Some(cleaned) = sanitize_cookie_input(&self.cookie_text) else {
            return false;
        };
        self.apply_session(&cleaned);
        self.cookie_text.clear();
        true
    }

    /// Attempts to decode and import a companion handoff code.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn import_handoff(&mut self) -> bool {
        let Ok(cleaned) = decode_handoff(&self.handoff_text) else {
            return false;
        };
        self.apply_session(&cleaned);
        self.handoff_text.clear();
        true
    }

    /// Clears the stored session and resets cookie and handoff input buffers.
    pub fn sign_out(&mut self) {
        self.session.clear();
        self.catalog.set_session_cookies(None);
        self.signed_in = false;
        self.cookie_text.clear();
        self.handoff_text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::infra::{
        AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore, encode_handoff,
    };

    #[test]
    fn sign_in_and_out_lifecycle_with_sanitized_input() {
        let mut state = wired();
        assert!(!state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), None);

        state.cookie_text = "  cookie: SID=secret123; HSID=secret456  ".to_owned();
        state.handoff_text = "leftover_handoff".to_owned();
        let ok = state.sign_in_from_input();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.cookie_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=secret123; HSID=secret456"));
        assert_eq!(state.catalog.last_session_cookies(), Some("SID=secret123; HSID=secret456"));

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
        assert_eq!(state.catalog.last_session_cookies(), None);
        assert_eq!(state.handoff_text, "");
    }

    #[test]
    fn sign_in_from_input_rejects_empty_or_whitespace() {
        let mut state = wired();
        for invalid in ["", "   ", "cookie:", "COOKIE:   "] {
            state.cookie_text = invalid.to_owned();
            let ok = state.sign_in_from_input();
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert!(!state.is_signed_in());
            assert_eq!(state.catalog.last_session_cookies(), None);
        }
    }

    #[test]
    fn import_handoff_success_applies_session_and_clears_buffer() {
        let mut state = wired();
        assert!(!state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), None);

        let code = encode_handoff("cookie: SID=companion123; HSID=companion456").expect("encode");
        state.handoff_text = code;

        let ok = state.import_handoff();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.handoff_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=companion123; HSID=companion456"));
        assert_eq!(
            state.catalog.last_session_cookies(),
            Some("SID=companion123; HSID=companion456")
        );

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
        assert_eq!(state.catalog.last_session_cookies(), None);
        assert_eq!(state.handoff_text, "");
        assert_eq!(state.cookie_text, "");
    }

    #[test]
    fn import_handoff_failure_leaves_state_untouched() {
        let mut state = wired();
        let invalid_cases = [
            "",
            "   ",
            "MG1.invalid_base64!.12345678",
            "corrupt_handoff_envelope",
            "MG2.Zm9v.12345678",
        ];

        for invalid in invalid_cases {
            state.handoff_text = invalid.to_owned();
            let ok = state.import_handoff();
            assert!(!ok, "expected '{invalid}' to fail");
            assert!(!state.is_signed_in());
            assert_eq!(state.catalog.last_session_cookies(), None);
            assert_eq!(state.session.load(), None);
            assert_eq!(state.handoff_text, invalid);
        }
    }

    #[test]
    fn startup_with_preloaded_session_store_forwards_cookies() {
        let mut session = MemorySessionStore::new();
        session.save("PRELOADED=session_cookie_123");
        let state = AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            session,
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        assert!(state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), Some("PRELOADED=session_cookie_123"));
    }
}
