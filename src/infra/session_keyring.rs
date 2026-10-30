//! OS-keyring session storage and cookie input sanitization.
//!
//! Provides persistence for authenticated YouTube Music session cookies
//! using the platform credential store (Secret Service on Linux, Credential
//! Manager on Windows, Keychain on macOS).
//!
//! # Provider-absent fallback (SDD D-2 fragmentation)
//! On systems without a configured or running secret provider (e.g. headless
//! Linux servers, minimal containers, or environments lacking D-Bus Secret
//! Service), keyring operations will fail softly with a warning log, and
//! `load()` will return `None`.

use super::SessionStore;

/// Pure helper that cleans pasted cookie headers.
///
/// Trims surrounding whitespace, strips an optional single case-insensitive
/// `cookie:` prefix, and rejects empty remainders.
pub fn sanitize_cookie_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let cleaned = strip_cookie_prefix(trimmed);
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_owned())
}

fn strip_cookie_prefix(value: &str) -> &str {
    if let Some(prefix) = value.get(..7) {
        if prefix.eq_ignore_ascii_case("cookie:") {
            return value.get(7..).unwrap_or("").trim_start();
        }
    }
    value
}

/// Session store backed by the platform credential manager via `keyring`.
#[derive(Debug, Clone)]
pub struct KeyringSessionStore {
    service: String,
    account: String,
}

impl KeyringSessionStore {
    /// Creates a store with custom service and account names.
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self { service: service.into(), account: account.into() }
    }

    /// Creates the default application store targeting YouTube Music cookies.
    pub fn moggyt() -> Self {
        Self::new("moggyt", "youtube-music-cookies")
    }

    fn entry(&self) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, &self.account).map_err(Into::into)
    }
}

impl SessionStore for KeyringSessionStore {
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn save(&mut self, cookies: &str) {
        let Some(cleaned) = sanitize_cookie_input(cookies) else {
            self.clear();
            return;
        };

        let entry = match self.entry() {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, "failed to open OS keyring entry for save");
                return;
            }
        };

        if let Err(err) = entry.set_secret(cleaned.as_bytes()) {
            tracing::warn!(error = %err, "failed to store session cookies in OS keyring");
        }
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn load(&self) -> Option<String> {
        let entry = match self.entry() {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, "failed to open OS keyring entry for load");
                return None;
            }
        };

        let bytes = match entry.get_secret() {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load session cookies from OS keyring");
                return None;
            }
        };

        match String::from_utf8(bytes) {
            Ok(stored) => Some(stored),
            Err(err) => {
                tracing::warn!(error = %err, "stored OS keyring secret is not valid UTF-8");
                None
            }
        }
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn clear(&mut self) {
        let entry = match self.entry() {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, "failed to open OS keyring entry for clear");
                return;
            }
        };

        if let Err(err) = entry.delete_credential() {
            if !matches!(err, keyring::Error::NoEntry) {
                tracing::warn!(error = %err, "failed to clear session cookies from OS keyring");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_cookie_input_table() {
        let cases = [
            ("SID=abc123xyz; HSID=def456", Some("SID=abc123xyz; HSID=def456")),
            ("  SID=abc123xyz  ", Some("SID=abc123xyz")),
            ("cookie: SID=abc123xyz", Some("SID=abc123xyz")),
            ("COOKIE:   SID=abc123xyz", Some("SID=abc123xyz")),
            ("Cookie:SID=abc123xyz", Some("SID=abc123xyz")),
            ("cOoKiE: \t SID=abc123xyz", Some("SID=abc123xyz")),
            ("", None),
            ("   ", None),
            ("\t\r\n", None),
            ("cookie:", None),
            ("COOKIE:   ", None),
            ("cookie:\t\n", None),
        ];

        for (input, expected) in cases {
            assert_eq!(
                sanitize_cookie_input(input).as_deref(),
                expected,
                "failed for input: {input:?}"
            );
        }
    }

    #[test]
    fn keyring_session_store_constructors() {
        let custom = KeyringSessionStore::new("custom-svc", "custom-acc");
        assert_eq!(custom.service, "custom-svc");
        assert_eq!(custom.account, "custom-acc");

        let default_store = KeyringSessionStore::moggyt();
        assert_eq!(default_store.service, "moggyt");
        assert_eq!(default_store.account, "youtube-music-cookies");
    }

    #[test]
    fn keyring_session_store_provider_tolerance_lifecycle() {
        let mut store = KeyringSessionStore::new("moggyt-test", "moggyt-test-probe");
        store.clear();

        store.save("cookie: SID=probe_test_cookie_123");
        match store.load() {
            Some(value) => {
                assert_eq!(value, "SID=probe_test_cookie_123");
                store.save("   ");
                assert_eq!(store.load(), None);
            }
            None => {
                eprintln!("keyring provider absent in current environment; tolerated per SDD D-2");
                assert_eq!(store.load(), None);
            }
        }

        store.clear();
    }
}
