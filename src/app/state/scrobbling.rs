//! Scrobbling integration and credentials management for [`AppState`].

use crate::app::AppState;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, CredentialStore, LastFmClient, LastFmCredentials,
    LyricsSource, Scrobbler, SessionStore, client_from_credentials, should_scrobble,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects an alternative scrobbler implementation.
    pub fn set_scrobbler(&mut self, scrobbler: Box<dyn Scrobbler>) {
        self.scrobbler = scrobbler;
    }

    /// Injects an alternative secondary scrobbler implementation.
    pub fn set_secondary_scrobbler(&mut self, scrobbler: Option<Box<dyn Scrobbler>>) {
        self.secondary_scrobbler = scrobbler;
    }

    /// Injects an alternative Last.fm credential store implementation.
    pub fn set_credential_store(&mut self, store: Box<dyn CredentialStore>) {
        self.credentials = store;
        self.sync_lastfm_scrobbler();
    }

    /// Injects an alternative secondary Last.fm credential store implementation.
    pub fn set_secondary_credential_store(&mut self, store: Box<dyn CredentialStore>) {
        self.secondary_credentials = store;
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Sets whether Last.fm live scrobbling is enabled and persists the preference.
    ///
    /// Disabling scrobbling switches to a dormant client immediately to prevent
    /// outgoing network requests, but preserves stored credentials in the vault
    /// unless explicitly cleared.
    pub fn set_lastfm_enabled(&mut self, enabled: bool) {
        self.lastfm_enabled = enabled;
        self.persist_settings();
        self.sync_lastfm_scrobbler();
    }

    pub(crate) fn sync_lastfm_scrobbler(&mut self) {
        if !self.lastfm_enabled {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        }
        let Some(creds) = self.credentials.load_lastfm() else {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        };
        if !creds.is_complete() {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        }
        self.scrobbler = Box::new(client_from_credentials(&creds));
    }

    /// Saves Last.fm credentials to the vault and synchronizes scrobbler state.
    pub fn save_lastfm_credentials(&mut self, creds: &LastFmCredentials) {
        self.credentials.save_lastfm(creds);
        self.sync_lastfm_scrobbler();
    }

    /// Clears persisted Last.fm credentials from the vault, input buffers, and resets scrobbler to dormant.
    pub fn clear_lastfm_credentials(&mut self) {
        self.credentials.clear_lastfm();
        self.lastfm_api_key_text.clear();
        self.lastfm_secret_text.clear();
        self.lastfm_session_key_text.clear();
        self.sync_lastfm_scrobbler();
    }

    /// Saves credentials from UI input buffers and clears buffers on completion.
    pub fn save_lastfm_credentials_from_input(&mut self) -> bool {
        let creds = LastFmCredentials::new(
            &self.lastfm_api_key_text,
            &self.lastfm_secret_text,
            &self.lastfm_session_key_text,
        );
        let complete = creds.is_complete();
        self.save_lastfm_credentials(&creds);
        self.lastfm_api_key_text.clear();
        self.lastfm_secret_text.clear();
        self.lastfm_session_key_text.clear();
        complete
    }

    /// Returns true if the active scrobbler has a live authenticated session.
    pub fn lastfm_is_live(&self) -> bool {
        self.scrobbler.as_any().downcast_ref::<LastFmClient>().is_some_and(LastFmClient::is_live)
    }

    /// Sets whether secondary Last.fm live scrobbling is enabled and persists the preference.
    pub fn set_secondary_lastfm_enabled(&mut self, enabled: bool) {
        self.secondary_lastfm_enabled = enabled;
        self.persist_settings();
        self.sync_secondary_lastfm_scrobbler();
    }

    pub(crate) fn sync_secondary_lastfm_scrobbler(&mut self) {
        if !self.secondary_lastfm_enabled {
            self.secondary_scrobbler = None;
            return;
        }
        let Some(creds) = self.secondary_credentials.load_lastfm() else {
            self.secondary_scrobbler = None;
            return;
        };
        if !creds.is_complete() {
            self.secondary_scrobbler = None;
            return;
        }
        self.secondary_scrobbler = Some(Box::new(client_from_credentials(&creds)));
    }

    /// Saves secondary Last.fm credentials to the vault and synchronizes secondary scrobbler state.
    pub fn save_secondary_lastfm_credentials(&mut self, creds: &LastFmCredentials) {
        self.secondary_credentials.save_lastfm(creds);
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Clears persisted secondary Last.fm credentials from the vault, input buffers, and resets secondary scrobbler to None.
    pub fn clear_secondary_lastfm_credentials(&mut self) {
        self.secondary_credentials.clear_lastfm();
        self.secondary_lastfm_api_key_text.clear();
        self.secondary_lastfm_secret_text.clear();
        self.secondary_lastfm_session_key_text.clear();
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Saves secondary credentials from UI input buffers and clears buffers on completion.
    pub fn save_secondary_lastfm_credentials_from_input(&mut self) -> bool {
        let creds = LastFmCredentials::new(
            &self.secondary_lastfm_api_key_text,
            &self.secondary_lastfm_secret_text,
            &self.secondary_lastfm_session_key_text,
        );
        let complete = creds.is_complete();
        self.save_secondary_lastfm_credentials(&creds);
        self.secondary_lastfm_api_key_text.clear();
        self.secondary_lastfm_secret_text.clear();
        self.secondary_lastfm_session_key_text.clear();
        complete
    }

    /// Returns true if the secondary scrobbler has a live authenticated session.
    pub fn secondary_lastfm_is_live(&self) -> bool {
        self.secondary_scrobbler
            .as_ref()
            .and_then(|s| s.as_any().downcast_ref::<LastFmClient>())
            .is_some_and(LastFmClient::is_live)
    }

    pub(crate) fn forward_now_playing(&mut self, artist: &str, title: &str) {
        self.scrobbler.now_playing(artist, title);
        if let Some(sec) = self.secondary_scrobbler.as_mut() {
            sec.now_playing(artist, title);
        }
    }

    fn forward_scrobble(&mut self, artist: &str, title: &str, timestamp: u64) {
        self.scrobbler.scrobble(artist, title, timestamp);
        if let Some(sec) = self.secondary_scrobbler.as_mut() {
            sec.scrobble(artist, title, timestamp);
        }
    }

    pub(crate) fn maybe_scrobble_current(&mut self) {
        let elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        self.maybe_scrobble_with_elapsed(elapsed);
    }

    pub(crate) fn maybe_scrobble_with_elapsed(&mut self, elapsed_secs: u64) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let duration = track.duration_secs.map(|d| d as u64);
        if !should_scrobble(elapsed_secs, duration) {
            return;
        }
        let timestamp = self.started_at.map_or(0, |(_, ts)| ts);
        let artist = track.artist.clone();
        let title = track.title.clone();
        self.forward_scrobble(&artist, &title, timestamp);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use crate::domain::Track;
    use crate::infra::{LastFmCredentials, StubScrobbler};

    #[test]
    fn scrobbler_now_playing_recorded_on_playback_funnel() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.toggle_play();

        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert_eq!(
            stub.now_playing_calls,
            vec![("Stub Artist".to_string(), "midnight".to_string())]
        );
    }

    #[test]
    fn scrobbler_advance_threshold_and_recording_lifecycle() {
        let mut state = wired();
        let track = Track::new_full("vid-1", "Karma Police", "Radiohead", None, Some(240));
        state.queue.push(track);
        state.play_current_quiet();

        // Advance immediately with ~0s elapsed -> no scrobble
        state.next_track();
        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert!(stub.scrobble_calls.is_empty());

        // With sufficient elapsed playtime, scrobble is recorded
        state.maybe_scrobble_with_elapsed(120);
        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert_eq!(stub.scrobble_calls.len(), 1);
        assert_eq!(stub.scrobble_calls[0].0, "Radiohead");
        assert_eq!(stub.scrobble_calls[0].1, "Karma Police");
    }

    #[test]
    fn test_enabling_lastfm_with_complete_creds_yields_live_client() {
        let mut state = wired();
        let creds = LastFmCredentials::new("api-key", "api-secret", "session-key");
        state.save_lastfm_credentials(&creds);
        assert!(!state.lastfm_is_live());

        state.set_lastfm_enabled(true);
        assert!(state.lastfm_enabled);
        assert!(state.lastfm_is_live());
        assert!(state.settings_store.load().lastfm_enabled);
    }

    #[test]
    fn test_disabling_lastfm_yields_dormant_client_and_preserves_stored_creds() {
        let mut state = wired();
        let creds = LastFmCredentials::new("api-key", "api-secret", "session-key");
        state.save_lastfm_credentials(&creds);
        state.set_lastfm_enabled(true);
        assert!(state.lastfm_is_live());

        state.set_lastfm_enabled(false);
        assert!(!state.lastfm_enabled);
        assert!(!state.lastfm_is_live());
        assert!(!state.settings_store.load().lastfm_enabled);
        // Disabling scrobbler clears the session client but keeps stored credentials in vault
        assert_eq!(state.credentials.load_lastfm(), Some(creds));
    }

    #[test]
    fn test_incomplete_creds_never_go_live() {
        let mut state = wired();
        state.set_lastfm_enabled(true);

        for (k, s, sk) in [
            ("", "secret", "sk"),
            ("key", "", "sk"),
            ("key", "secret", ""),
            ("   ", "secret", "sk"),
            ("key", "   ", "sk"),
            ("key", "secret", "   "),
        ] {
            let incomplete = LastFmCredentials::new(k, s, sk);
            state.save_lastfm_credentials(&incomplete);
            assert!(
                !state.lastfm_is_live(),
                "expected incomplete creds ({k}, {s}, {sk}) to remain dormant"
            );
        }
    }

    #[test]
    fn test_lastfm_credentials_save_clear_round_trip() {
        let mut state = wired();
        assert_eq!(state.credentials.load_lastfm(), None);

        let creds = LastFmCredentials::new("my-key", "my-secret", "my-session");
        state.save_lastfm_credentials(&creds);
        assert_eq!(state.credentials.load_lastfm(), Some(creds));

        state.clear_lastfm_credentials();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert!(!state.lastfm_is_live());
    }

    #[test]
    fn test_lastfm_credentials_from_input_buffers() {
        let mut state = wired();
        state.lastfm_api_key_text = "test-key".into();
        state.lastfm_secret_text = "test-secret".into();
        state.lastfm_session_key_text = "test-session".into();
        state.set_lastfm_enabled(true);

        let ok = state.save_lastfm_credentials_from_input();
        assert!(ok);
        assert!(state.lastfm_is_live());
        assert!(state.lastfm_api_key_text.is_empty());
        assert!(state.lastfm_secret_text.is_empty());
        assert!(state.lastfm_session_key_text.is_empty());

        state.clear_lastfm_credentials();
        assert!(!state.lastfm_is_live());
        assert_eq!(state.credentials.load_lastfm(), None);
    }

    #[test]
    fn test_dual_scrobbling_forwards_events_to_both_scrobblers_with_identical_args() {
        let mut state = wired();
        let primary_stub = Box::new(StubScrobbler::new());
        let secondary_stub = Box::new(StubScrobbler::new());

        state.set_scrobbler(primary_stub);
        state.set_secondary_scrobbler(Some(secondary_stub));

        let track = Track::new_full("vid-dual", "Paranoid Android", "Radiohead", None, Some(380));
        state.queue.push(track);
        state.play_current_quiet();

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        let sec = state
            .secondary_scrobbler
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<StubScrobbler>()
            .unwrap();

        assert_eq!(prim.now_playing_calls, vec![("Radiohead".into(), "Paranoid Android".into())]);
        assert_eq!(sec.now_playing_calls, vec![("Radiohead".into(), "Paranoid Android".into())]);

        state.maybe_scrobble_with_elapsed(200);

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        let sec = state
            .secondary_scrobbler
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<StubScrobbler>()
            .unwrap();

        assert_eq!(prim.scrobble_calls.len(), 1);
        assert_eq!(sec.scrobble_calls.len(), 1);
        assert_eq!(prim.scrobble_calls[0].0, "Radiohead");
        assert_eq!(prim.scrobble_calls[0].1, "Paranoid Android");
        assert_eq!(sec.scrobble_calls[0].0, "Radiohead");
        assert_eq!(sec.scrobble_calls[0].1, "Paranoid Android");
        assert_eq!(prim.scrobble_calls[0].2, sec.scrobble_calls[0].2);
    }

    #[test]
    fn test_secondary_none_keeps_primary_only_behavior() {
        let mut state = wired();
        let primary_stub = Box::new(StubScrobbler::new());
        state.set_scrobbler(primary_stub);
        state.set_secondary_scrobbler(None);

        assert!(state.secondary_scrobbler.is_none());
        assert!(!state.secondary_lastfm_is_live());

        let track = Track::new_full("vid-single", "Karma Police", "Radiohead", None, Some(240));
        state.queue.push(track);
        state.play_current_quiet();
        state.maybe_scrobble_with_elapsed(150);

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        assert_eq!(prim.now_playing_calls, vec![("Radiohead".into(), "Karma Police".into())]);
        assert_eq!(prim.scrobble_calls.len(), 1);
        assert_eq!(prim.scrobble_calls[0].0, "Radiohead");
        assert_eq!(prim.scrobble_calls[0].1, "Karma Police");
        assert!(state.secondary_scrobbler.is_none());
    }

    #[test]
    fn test_enabling_secondary_lastfm_with_complete_creds_yields_live_secondary() {
        let mut state = wired();
        let creds = LastFmCredentials::new("sec-key", "sec-secret", "sec-session");
        state.save_secondary_lastfm_credentials(&creds);
        assert!(!state.secondary_lastfm_is_live());
        assert!(state.secondary_scrobbler.is_none());

        state.set_secondary_lastfm_enabled(true);
        assert!(state.secondary_lastfm_enabled);
        assert!(state.secondary_lastfm_is_live());
        assert!(state.settings_store.load().secondary_lastfm_enabled);

        state.set_secondary_lastfm_enabled(false);
        assert!(!state.secondary_lastfm_enabled);
        assert!(!state.secondary_lastfm_is_live());
        assert!(state.secondary_scrobbler.is_none());
        assert!(!state.settings_store.load().secondary_lastfm_enabled);
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(creds));
    }

    #[test]
    fn test_incomplete_secondary_creds_never_go_live() {
        let mut state = wired();
        state.set_secondary_lastfm_enabled(true);

        for (k, s, sk) in [
            ("", "secret", "sk"),
            ("key", "", "sk"),
            ("key", "secret", ""),
            ("   ", "secret", "sk"),
            ("key", "   ", "sk"),
            ("key", "secret", "   "),
        ] {
            let incomplete = LastFmCredentials::new(k, s, sk);
            state.save_secondary_lastfm_credentials(&incomplete);
            assert!(
                !state.secondary_lastfm_is_live(),
                "expected incomplete secondary creds ({k}, {s}, {sk}) to remain dormant"
            );
            assert!(state.secondary_scrobbler.is_none());
        }
    }

    #[test]
    fn test_secondary_credentials_save_clear_round_trip_and_namespace_separation() {
        let mut state = wired();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert_eq!(state.secondary_credentials.load_lastfm(), None);

        let primary_creds = LastFmCredentials::new("prim-key", "prim-sec", "prim-session");
        let secondary_creds = LastFmCredentials::new("sec-key", "sec-sec", "sec-session");

        // Primary save does not satisfy secondary load
        state.save_lastfm_credentials(&primary_creds);
        assert_eq!(state.credentials.load_lastfm(), Some(primary_creds.clone()));
        assert_eq!(state.secondary_credentials.load_lastfm(), None);

        // Secondary save does not overwrite primary
        state.save_secondary_lastfm_credentials(&secondary_creds);
        assert_eq!(state.credentials.load_lastfm(), Some(primary_creds));
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(secondary_creds.clone()));

        // Clearing primary leaves secondary intact
        state.clear_lastfm_credentials();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(secondary_creds));

        // Clearing secondary leaves both empty
        state.clear_secondary_lastfm_credentials();
        assert_eq!(state.secondary_credentials.load_lastfm(), None);
        assert!(!state.secondary_lastfm_is_live());
    }

    #[test]
    fn test_secondary_credentials_from_input_buffers() {
        let mut state = wired();
        state.secondary_lastfm_api_key_text = "sec-input-key".into();
        state.secondary_lastfm_secret_text = "sec-input-sec".into();
        state.secondary_lastfm_session_key_text = "sec-input-session".into();
        state.set_secondary_lastfm_enabled(true);

        let ok = state.save_secondary_lastfm_credentials_from_input();
        assert!(ok);
        assert!(state.secondary_lastfm_is_live());
        assert!(state.secondary_lastfm_api_key_text.is_empty());
        assert!(state.secondary_lastfm_secret_text.is_empty());
        assert!(state.secondary_lastfm_session_key_text.is_empty());

        state.clear_secondary_lastfm_credentials();
        assert!(!state.secondary_lastfm_is_live());
        assert_eq!(state.secondary_credentials.load_lastfm(), None);
    }
}
