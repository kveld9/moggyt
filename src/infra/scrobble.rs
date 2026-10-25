//! Scrobbler port, credential vault, and Last.fm 2.0 API client.
//!
//! Exposes the [`Scrobbler`] trait for playback progress reporting,
//! credential persistence via [`CredentialStore`], pure Last.fm rule helpers,
//! and a dormant client awaiting credentials.

use md5::{Digest, Md5};

/// Public endpoint for Last.fm 2.0 API requests.
pub const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

/// Port for publishing playback status and recording scrobbles.
pub trait Scrobbler: Send {
    /// Notifies the scrobbler that a new track has begun playing.
    fn now_playing(&mut self, artist: &str, title: &str);

    /// Submits a completed scrobble record for a track.
    fn scrobble(&mut self, artist: &str, title: &str, timestamp_secs: u64);

    /// Downcasts the trait object to a concrete type for test verification.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// In-memory recording stub for scrobble assertions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StubScrobbler {
    /// Recorded `(artist, title)` pairs from `now_playing` calls.
    pub now_playing_calls: Vec<(String, String)>,
    /// Recorded `(artist, title, timestamp_secs)` tuples from `scrobble` calls.
    pub scrobble_calls: Vec<(String, String, u64)>,
}

impl StubScrobbler {
    /// Creates an empty recording stub.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Scrobbler for StubScrobbler {
    fn now_playing(&mut self, artist: &str, title: &str) {
        self.now_playing_calls.push((artist.to_string(), title.to_string()));
    }

    fn scrobble(&mut self, artist: &str, title: &str, timestamp_secs: u64) {
        self.scrobble_calls.push((artist.to_string(), title.to_string(), timestamp_secs));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Last.fm 2.0 API client for scrobbling and now-playing status updates.
pub struct LastFmClient {
    client: reqwest::blocking::Client,
    api_key: String,
    secret: String,
    session_key: Option<String>,
}

impl LastFmClient {
    /// Constructs a client with API key and shared secret.
    pub fn new(api_key: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            api_key: api_key.into(),
            secret: secret.into(),
            session_key: None,
        }
    }

    /// Attaches an authenticated Last.fm session key.
    pub fn with_session_key(mut self, session_key: impl Into<String>) -> Self {
        self.session_key = Some(session_key.into());
        self
    }

    /// Returns true when API key, secret, and session key are all non-blank.
    pub fn is_live(&self) -> bool {
        if self.api_key.trim().is_empty() || self.secret.trim().is_empty() {
            return false;
        }
        self.session_key.as_deref().is_some_and(|sk| !sk.trim().is_empty())
    }

    fn active_session(&self) -> Option<&str> {
        if !self.is_live() {
            return None;
        }
        self.session_key.as_deref()
    }

    fn send_signed_post(&self, params: &[(&str, &str)]) {
        let sig = api_signature(params, &self.secret);
        let mut form = params.to_vec();
        form.push(("api_sig", &sig));
        form.push(("format", "json"));
        if let Err(err) = self.client.post(LASTFM_API_URL).form(&form).send() {
            tracing::warn!("Last.fm request failed: {err}");
        }
    }
}

impl Scrobbler for LastFmClient {
    fn now_playing(&mut self, artist: &str, title: &str) {
        let Some(sk) = self.active_session() else {
            return;
        };
        let params = [
            ("api_key", self.api_key.as_str()),
            ("artist", artist),
            ("method", "track.updateNowPlaying"),
            ("sk", sk),
            ("track", title),
        ];
        self.send_signed_post(&params);
    }

    fn scrobble(&mut self, artist: &str, title: &str, timestamp_secs: u64) {
        let Some(sk) = self.active_session() else {
            return;
        };
        let ts_str = timestamp_secs.to_string();
        let params = [
            ("api_key", self.api_key.as_str()),
            ("artist", artist),
            ("method", "track.scrobble"),
            ("sk", sk),
            ("timestamp", ts_str.as_str()),
            ("track", title),
        ];
        self.send_signed_post(&params);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Last.fm scrobble eligibility predicate.
///
/// Returns true only when:
/// 1. Track duration is known and >= 30 seconds (Last.fm rule).
/// 2. Played duration >= min(duration / 2, 240 seconds).
pub fn should_scrobble(played_secs: u64, duration_secs: Option<u64>) -> bool {
    let Some(duration) = duration_secs else {
        return false;
    };
    if duration < 30 {
        return false;
    }
    let threshold = (duration / 2).min(240);
    played_secs >= threshold
}

/// Computes the Last.fm API method signature (`api_sig`).
///
/// Parameters are sorted alphabetically by key, concatenated as `keyvalue`,
/// filtered to exclude the `format` parameter, suffixed with the API secret,
/// and hashed using MD5 (represented as a 32-character lowercase hex string).
pub fn api_signature(params: &[(&str, &str)], secret: &str) -> String {
    let mut filtered: Vec<(&str, &str)> =
        params.iter().copied().filter(|(k, _)| *k != "format").collect();
    filtered.sort_by_key(|(k, _)| *k);
    let mut buffer = String::new();
    for (k, v) in filtered {
        buffer.push_str(k);
        buffer.push_str(v);
    }
    buffer.push_str(secret);
    let digest = Md5::digest(buffer.as_bytes());
    format!("{digest:x}")
}

/// Last.fm API credentials and user session key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastFmCredentials {
    pub api_key: String,
    pub secret: String,
    pub session_key: String,
}

impl LastFmCredentials {
    /// Constructs a credential bundle.
    pub fn new(
        api_key: impl Into<String>,
        secret: impl Into<String>,
        session_key: impl Into<String>,
    ) -> Self {
        Self { api_key: api_key.into(), secret: secret.into(), session_key: session_key.into() }
    }

    /// Returns true when API key, secret, and session key are all non-empty after trimming.
    pub fn is_complete(&self) -> bool {
        if self.api_key.trim().is_empty() || self.secret.trim().is_empty() {
            return false;
        }
        !self.session_key.trim().is_empty()
    }
}

/// Constructs a [`LastFmClient`] initialized with the given credentials.
pub fn client_from_credentials(creds: &LastFmCredentials) -> LastFmClient {
    LastFmClient::new(&creds.api_key, &creds.secret).with_session_key(&creds.session_key)
}

/// Port for persisting and retrieving Last.fm credentials.
pub trait CredentialStore: Send + Sync {
    /// Saves Last.fm credentials, or clears them when incomplete.
    fn save_lastfm(&mut self, creds: &LastFmCredentials);
    /// Loads persisted credentials if all three fields are present and non-empty.
    fn load_lastfm(&self) -> Option<LastFmCredentials>;
    /// Clears any persisted Last.fm credentials.
    fn clear_lastfm(&mut self);
}

/// In-memory credential store for tests and transient sessions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MemoryCredentialStore {
    inner: Option<LastFmCredentials>,
}

impl MemoryCredentialStore {
    /// Creates an empty memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn save_lastfm(&mut self, creds: &LastFmCredentials) {
        self.inner = creds.is_complete().then(|| creds.clone());
    }

    fn load_lastfm(&self) -> Option<LastFmCredentials> {
        self.inner.as_ref().filter(|c| c.is_complete()).cloned()
    }

    fn clear_lastfm(&mut self) {
        self.inner = None;
    }
}

/// Credential store backed by the platform credential manager via `keyring`.
#[derive(Debug, Clone)]
pub struct KeyringCredentialStore {
    service: String,
    api_key_account: String,
    secret_account: String,
    session_account: String,
}

impl KeyringCredentialStore {
    /// Creates a store with custom service and account names.
    pub fn new(
        service: impl Into<String>,
        api_key_account: impl Into<String>,
        secret_account: impl Into<String>,
        session_account: impl Into<String>,
    ) -> Self {
        Self {
            service: service.into(),
            api_key_account: api_key_account.into(),
            secret_account: secret_account.into(),
            session_account: session_account.into(),
        }
    }

    /// Creates the default application store targeting Last.fm credentials.
    pub fn moggyt() -> Self {
        Self::new("moggyt", "lastfm-api-key", "lastfm-secret", "lastfm-session-key")
    }

    fn entry(&self, account: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account).map_err(Into::into)
    }

    fn open_entry(&self, account: &str, op: &str) -> Option<keyring::Entry> {
        self.entry(account)
            .map_err(|err| tracing::warn!(error = %err, "failed to open OS keyring entry for {op}"))
            .ok()
    }

    fn save_entry(&self, account: &str, secret: &str) -> bool {
        let Some(entry) = self.open_entry(account, "save") else {
            return false;
        };
        if let Err(err) = entry.set_secret(secret.as_bytes()) {
            tracing::warn!(error = %err, "failed to store Last.fm credential in OS keyring");
        }
        true
    }

    fn load_entry(&self, account: &str) -> Option<String> {
        let entry = self.open_entry(account, "load")?;
        let bytes = entry
            .get_secret()
            .map_err(|err| {
                tracing::warn!(error = %err, "failed to load Last.fm credential from OS keyring");
            })
            .ok()?;
        match String::from_utf8(bytes) {
            Ok(stored) => Some(stored),
            Err(err) => {
                tracing::warn!(error = %err, "stored OS keyring secret is not valid UTF-8");
                None
            }
        }
    }

    fn clear_entry(&self, account: &str) {
        let Some(entry) = self.open_entry(account, "clear") else {
            return;
        };
        if let Err(err) = entry.delete_credential() {
            if !matches!(err, keyring::Error::NoEntry) {
                tracing::warn!(error = %err, "failed to clear Last.fm credential from OS keyring");
            }
        }
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn save_lastfm(&mut self, creds: &LastFmCredentials) {
        if !creds.is_complete() {
            self.clear_lastfm();
            return;
        }
        if !self.save_entry(&self.api_key_account, &creds.api_key)
            || !self.save_entry(&self.secret_account, &creds.secret)
        {
            return;
        }
        self.save_entry(&self.session_account, &creds.session_key);
    }

    fn load_lastfm(&self) -> Option<LastFmCredentials> {
        let creds = LastFmCredentials::new(
            self.load_entry(&self.api_key_account)?,
            self.load_entry(&self.secret_account)?,
            self.load_entry(&self.session_account)?,
        );
        creds.is_complete().then_some(creds)
    }

    fn clear_lastfm(&mut self) {
        for account in [&self.api_key_account, &self.secret_account, &self.session_account] {
            self.clear_entry(account);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lastfm_client_is_live_and_dormant_calls() {
        let dormant = LastFmClient::new("", "");
        assert!(!dormant.is_live());

        let with_creds = LastFmClient::new("key", "secret");
        assert!(!with_creds.is_live());

        let with_empty_sk = with_creds.with_session_key("   ");
        assert!(!with_empty_sk.is_live());

        let live = LastFmClient::new("key", "secret").with_session_key("sk_valid");
        assert!(live.is_live());

        // Dormant client does not panic on Scrobbler calls
        let mut client = LastFmClient::new("", "");
        client.now_playing("Radiohead", "Karma Police");
        client.scrobble("Radiohead", "Karma Police", 1700000000);
    }

    #[test]
    fn stub_scrobbler_records_calls() {
        let mut stub = StubScrobbler::new();
        assert!(stub.now_playing_calls.is_empty());
        assert!(stub.scrobble_calls.is_empty());

        stub.now_playing("Radiohead", "Karma Police");
        stub.scrobble("Radiohead", "Karma Police", 1700000000);

        assert_eq!(
            stub.now_playing_calls,
            vec![("Radiohead".to_string(), "Karma Police".to_string())]
        );
        assert_eq!(
            stub.scrobble_calls,
            vec![("Radiohead".to_string(), "Karma Police".to_string(), 1700000000)]
        );
    }

    #[test]
    fn should_scrobble_table() {
        // (played_secs, duration_secs, expected)
        let cases = [
            (100, None, false),
            (29, Some(29), false),
            (14, Some(30), false),
            (15, Some(30), true),
            (239, Some(600), false),
            (240, Some(600), true),
            (300, Some(600), true),
        ];

        for (played, dur, expected) in cases {
            assert_eq!(should_scrobble(played, dur), expected, "played={played}, dur={dur:?}");
        }
    }

    #[test]
    fn api_signature_sorts_omits_format_and_hashes() {
        // Precomputed via shell: printf "a1b2s" | md5sum
        // -> 1d0396bcbc2c54e569e7af9cf9c4685e
        let expected = "1d0396bcbc2c54e569e7af9cf9c4685e";
        let params = [("a", "1"), ("b", "2")];
        assert_eq!(api_signature(&params, "s"), expected);

        // Parameters with format and unsorted order must yield identical signature
        let params_with_format = [("b", "2"), ("format", "json"), ("a", "1")];
        assert_eq!(api_signature(&params_with_format, "s"), expected);
    }

    #[test]
    fn lastfm_credentials_and_client_construction() {
        let creds = LastFmCredentials::new("key", "sec", "sk");
        assert!(creds.is_complete());
        assert!(client_from_credentials(&creds).is_live());

        for (k, s, sk) in [("", "sec", "sk"), ("key", " ", "sk"), ("key", "sec", "\t")] {
            let item = LastFmCredentials::new(k, s, sk);
            assert!(!item.is_complete());
            assert!(!client_from_credentials(&item).is_live());
        }
    }

    #[test]
    fn memory_credential_store_lifecycle() {
        let mut store = MemoryCredentialStore::new();
        assert_eq!(store.load_lastfm(), None);

        let creds = LastFmCredentials::new("key", "sec", "sk");
        store.save_lastfm(&creds);
        assert_eq!(store.load_lastfm(), Some(creds.clone()));

        // Incomplete save clears existing credentials; blank fields return None
        store.save_lastfm(&LastFmCredentials::new("key", "  ", "sk"));
        assert_eq!(store.load_lastfm(), None);

        store.save_lastfm(&creds);
        store.clear_lastfm();
        assert_eq!(store.load_lastfm(), None);
    }

    #[test]
    fn keyring_credential_store_constructors() {
        let custom = KeyringCredentialStore::new("svc", "k", "s", "sk");
        assert_eq!(custom.service, "svc");
        assert_eq!(custom.api_key_account, "k");
        assert_eq!(custom.secret_account, "s");
        assert_eq!(custom.session_account, "sk");

        let def = KeyringCredentialStore::moggyt();
        assert_eq!(def.service, "moggyt");
        assert_eq!(def.api_key_account, "lastfm-api-key");
        assert_eq!(def.secret_account, "lastfm-secret");
        assert_eq!(def.session_account, "lastfm-session-key");
    }

    #[test]
    fn keyring_credential_store_provider_tolerance_lifecycle() {
        let mut store = KeyringCredentialStore::new(
            "moggyt-test",
            "moggyt-test-lastfm-api-key",
            "moggyt-test-lastfm-secret",
            "moggyt-test-lastfm-session-key",
        );
        store.clear_lastfm();
        let creds = LastFmCredentials::new("test-key", "test-secret", "test-session");
        store.save_lastfm(&creds);

        match store.load_lastfm() {
            Some(loaded) => {
                assert_eq!(loaded, creds);
                store.save_lastfm(&LastFmCredentials::new("test-key", "   ", "test-session"));
                assert_eq!(store.load_lastfm(), None);
            }
            None => {
                eprintln!("keyring provider absent in current environment; tolerated per SDD D-2")
            }
        }
        store.clear_lastfm();
    }
}
