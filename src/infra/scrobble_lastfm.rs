//! Last.fm 2.0 API client, credential types, and request signing helpers.

use md5::{Digest, Md5};

use super::scrobble::Scrobbler;

/// Public endpoint for Last.fm 2.0 API requests.
pub const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

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

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
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
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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
}
