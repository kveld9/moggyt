//! In-memory and OS keyring credential store implementations for Last.fm.

use super::scrobble::CredentialStore;
use super::scrobble_lastfm::LastFmCredentials;

/// In-memory credential store for tests and transient sessions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MemoryCredentialStore {
    inner: Option<LastFmCredentials>,
    namespace: String,
}

impl MemoryCredentialStore {
    /// Creates an empty memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty memory store under a designated namespace.
    pub fn with_namespace(namespace: impl Into<String>) -> Self {
        Self { inner: None, namespace: namespace.into() }
    }

    /// Returns the namespace of this store.
    pub fn namespace(&self) -> &str {
        &self.namespace
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

    /// Creates the application store targeting secondary Last.fm credentials.
    ///
    /// The secondary store shares the "moggyt" service namespace with the primary
    /// store, but uses distinct account names (`lastfm-secondary-api-key`,
    /// `lastfm-secondary-secret`, `lastfm-secondary-session-key`). This groups all
    /// moggyt credentials cleanly in the OS credential manager while guaranteeing
    /// strict credential isolation between primary and secondary scrobblers.
    pub fn moggyt_secondary() -> Self {
        Self::new(
            "moggyt",
            "lastfm-secondary-api-key",
            "lastfm-secondary-secret",
            "lastfm-secondary-session-key",
        )
    }

    fn entry(&self, account: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account).map_err(Into::into)
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn open_entry(&self, account: &str, op: &str) -> Option<keyring::Entry> {
        self.entry(account)
            .map_err(|err| tracing::warn!(error = %err, "failed to open OS keyring entry for {op}"))
            .ok()
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn save_entry(&self, account: &str, secret: &str) -> bool {
        let Some(entry) = self.open_entry(account, "save") else {
            return false;
        };
        if let Err(err) = entry.set_secret(secret.as_bytes()) {
            tracing::warn!(error = %err, "failed to store Last.fm credential in OS keyring");
        }
        true
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
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

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
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

        let sec = KeyringCredentialStore::moggyt_secondary();
        assert_eq!(sec.service, "moggyt");
        assert_eq!(sec.api_key_account, "lastfm-secondary-api-key");
        assert_eq!(sec.secret_account, "lastfm-secondary-secret");
        assert_eq!(sec.session_account, "lastfm-secondary-session-key");
    }

    #[test]
    fn memory_credential_store_with_namespace() {
        let store = MemoryCredentialStore::with_namespace("test-secondary");
        assert_eq!(store.namespace(), "test-secondary");
        assert_eq!(store.load_lastfm(), None);
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
