//! OS keyring and in-memory credential storage for Spotify Web API access.
//!
//! Stores Spotify Client ID and Client Secret in the platform credential manager
//! (Secret Service on Linux, Credential Manager on Windows, Keychain on macOS).
//! In headless, containerized, or minimal environments lacking a credential provider,
//! keyring operations fail softly with warning logs, and `load()` returns `None`.

use anyhow::Context;
use std::sync::Mutex;

/// Spotify Web API client credentials bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotifyCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl SpotifyCredentials {
    /// Constructs a credential bundle.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self { client_id: client_id.into(), client_secret: client_secret.into() }
    }

    /// Returns `true` when both client_id and client_secret are non-empty after trimming.
    pub fn is_complete(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
    }
}

/// Port trait for storing and retrieving Spotify Web API client credentials.
pub trait SpotifyCredentialStore: Send + Sync {
    /// Loads complete credentials, or returns `None` if absent or incomplete.
    fn load(&self) -> Option<SpotifyCredentials>;

    /// Saves credentials. If credentials are incomplete, clears existing credentials and returns `Ok(())`.
    fn save(&self, credentials: &SpotifyCredentials) -> anyhow::Result<()>;

    /// Clears any stored credentials.
    fn clear(&self) -> anyhow::Result<()>;
}

/// In-memory credential store backed by a thread-safe mutex for tests and transient sessions.
#[derive(Debug, Default)]
pub struct MemorySpotifyCredentialStore {
    inner: Mutex<Option<SpotifyCredentials>>,
}

impl MemorySpotifyCredentialStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SpotifyCredentialStore for MemorySpotifyCredentialStore {
    fn load(&self) -> Option<SpotifyCredentials> {
        let guard = self.inner.lock().ok()?;
        guard.as_ref().filter(|c| c.is_complete()).cloned()
    }

    fn save(&self, credentials: &SpotifyCredentials) -> anyhow::Result<()> {
        let mut guard =
            self.inner.lock().map_err(|err| anyhow::anyhow!("mutex lock poisoned: {err}"))?;
        if credentials.is_complete() {
            *guard = Some(credentials.clone());
        } else {
            *guard = None;
        }
        Ok(())
    }

    fn clear(&self) -> anyhow::Result<()> {
        let mut guard =
            self.inner.lock().map_err(|err| anyhow::anyhow!("mutex lock poisoned: {err}"))?;
        *guard = None;
        Ok(())
    }
}

/// Credential store backed by the platform credential manager via `keyring`.
#[derive(Debug, Clone)]
pub struct KeyringSpotifyCredentialStore {
    service: String,
    id_account: String,
    secret_account: String,
}

impl KeyringSpotifyCredentialStore {
    /// Creates a store with custom service and account names.
    pub fn new(
        service: impl Into<String>,
        id_account: impl Into<String>,
        secret_account: impl Into<String>,
    ) -> Self {
        Self {
            service: service.into(),
            id_account: id_account.into(),
            secret_account: secret_account.into(),
        }
    }

    /// Creates the default application store targeting Spotify credentials.
    pub fn moggyt() -> Self {
        Self::new("moggyt", "spotify-client-id", "spotify-client-secret")
    }

    fn entry(&self, account: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account).with_context(|| {
            format!("failed to initialize OS keyring entry for account '{account}'")
        })
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn set_secret_entry(&self, account: &str, secret: &str) -> anyhow::Result<()> {
        let entry = self.entry(account)?;
        entry.set_secret(secret.as_bytes()).map_err(|err| {
            tracing::warn!(error = %err, "failed to store Spotify credential in OS keyring");
            anyhow::anyhow!("failed to store Spotify credential for account '{account}': {err}")
        })
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn load_secret_entry(&self, account: &str) -> Option<String> {
        let entry = match self.entry(account) {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(error = %err, "failed to open OS keyring entry for load");
                return None;
            }
        };
        let bytes = match entry.get_secret() {
            Ok(bytes) => bytes,
            Err(err) => {
                if !matches!(err, keyring::Error::NoEntry) {
                    tracing::warn!(error = %err, "failed to load Spotify credential from OS keyring");
                }
                return None;
            }
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => {
                tracing::warn!(error = %err, "stored OS keyring secret is not valid UTF-8");
                return None;
            }
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(text)
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn delete_secret_entry(&self, account: &str) -> anyhow::Result<()> {
        let entry = self.entry(account)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => {
                tracing::warn!(error = %err, "failed to clear Spotify credential from OS keyring");
                Err(anyhow::anyhow!(
                    "failed to clear Spotify credential for account '{account}': {err}"
                ))
            }
        }
    }
}

impl SpotifyCredentialStore for KeyringSpotifyCredentialStore {
    fn load(&self) -> Option<SpotifyCredentials> {
        let client_id = self.load_secret_entry(&self.id_account)?;
        let client_secret = self.load_secret_entry(&self.secret_account)?;
        let credentials = SpotifyCredentials::new(client_id, client_secret);
        credentials.is_complete().then_some(credentials)
    }

    fn save(&self, credentials: &SpotifyCredentials) -> anyhow::Result<()> {
        if !credentials.is_complete() {
            return self.clear();
        }
        self.set_secret_entry(&self.id_account, &credentials.client_id)?;
        self.set_secret_entry(&self.secret_account, &credentials.client_secret)?;
        Ok(())
    }

    fn clear(&self) -> anyhow::Result<()> {
        self.delete_secret_entry(&self.id_account)?;
        self.delete_secret_entry(&self.secret_account)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_completeness_table() {
        let valid = SpotifyCredentials::new("client_id_123", "client_secret_456");
        assert!(valid.is_complete());

        let cases = [
            ("", "secret"),
            ("   ", "secret"),
            ("\t\n", "secret"),
            ("id", ""),
            ("id", "   "),
            ("id", "\t\n"),
            ("", ""),
            ("   ", "   "),
        ];

        for (id, secret) in cases {
            let creds = SpotifyCredentials::new(id, secret);
            assert!(!creds.is_complete(), "expected incomplete for id={id:?}, secret={secret:?}");
        }
    }

    #[test]
    fn memory_store_lifecycle_and_incomplete_clears() {
        let store = MemorySpotifyCredentialStore::new();
        assert_eq!(store.load(), None);

        let creds = SpotifyCredentials::new("test_id", "test_secret");
        assert!(store.save(&creds).is_ok());
        assert_eq!(store.load(), Some(creds.clone()));

        // Incomplete save clears existing credentials
        let incomplete = SpotifyCredentials::new("test_id", "   ");
        assert!(store.save(&incomplete).is_ok());
        assert_eq!(store.load(), None);

        // Re-save and explicit clear
        assert!(store.save(&creds).is_ok());
        assert_eq!(store.load(), Some(creds));
        assert!(store.clear().is_ok());
        assert_eq!(store.load(), None);
    }

    #[test]
    fn memory_store_poisoned_lock_behavior() {
        let store = MemorySpotifyCredentialStore::new();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = store.inner.lock().expect("mutex lock succeeds");
            panic!("poisoning test lock intentionally");
        }));
        assert_eq!(store.load(), None);
        assert!(store.save(&SpotifyCredentials::new("id", "secret")).is_err());
        assert!(store.clear().is_err());
    }

    #[test]
    fn keyring_store_constructor_fields() {
        let custom = KeyringSpotifyCredentialStore::new("custom_svc", "custom_id", "custom_secret");
        assert_eq!(custom.service, "custom_svc");
        assert_eq!(custom.id_account, "custom_id");
        assert_eq!(custom.secret_account, "custom_secret");

        let default_app = KeyringSpotifyCredentialStore::moggyt();
        assert_eq!(default_app.service, "moggyt");
        assert_eq!(default_app.id_account, "spotify-client-id");
        assert_eq!(default_app.secret_account, "spotify-client-secret");
    }

    #[test]
    fn keyring_store_provider_tolerance_lifecycle() {
        let store = KeyringSpotifyCredentialStore::new(
            "moggyt-test",
            "moggyt-test-spotify-client-id",
            "moggyt-test-spotify-client-secret",
        );
        let _ = store.clear();
        let creds = SpotifyCredentials::new("test-client-id-val", "test-client-secret-val");
        if store.save(&creds).is_err() {
            eprintln!("keyring provider absent in current environment; tolerated per SDD D-2");
            return;
        }

        match store.load() {
            Some(loaded) => {
                assert_eq!(loaded, creds);
                let incomplete = SpotifyCredentials::new("test-client-id-val", "   ");
                assert!(store.save(&incomplete).is_ok());
                assert_eq!(store.load(), None);
            }
            None => {
                eprintln!("keyring provider absent in current environment; tolerated per SDD D-2");
            }
        }
        let _ = store.clear();
    }
}
