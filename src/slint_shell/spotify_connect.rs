//! Spotify Web API credential storage, status derivation, and Slint UI callback wiring.
//!
//! Hexagonal invariant: pure status derivation and in-memory credential validation
//! separated from OS keyring persistence.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::i18n::{Locale, tr};
use crate::infra::{KeyringSpotifyCredentialStore, SpotifyCredentialStore, SpotifyCredentials};
use crate::slint_shell::state::SharedState;
use crate::slint_shell::{Integrations, MoggytWindow};

/// Computes localized status text for Spotify Web API integration display.
pub(crate) fn spotify_status_text(locale: Locale, configured: bool) -> String {
    if configured {
        tr(locale, "Spotify Web API: configured").to_string()
    } else {
        tr(locale, "Spotify Web API: not configured").to_string()
    }
}

fn parse_credentials(client_id: &str, client_secret: &str) -> Option<SpotifyCredentials> {
    let credentials = SpotifyCredentials::new(client_id.trim(), client_secret.trim());
    credentials.is_complete().then_some(credentials)
}

fn refresh_spotify_globals(
    window: &MoggytWindow,
    store: &dyn SpotifyCredentialStore,
    locale: Locale,
) {
    let integrations = window.global::<Integrations>();
    let configured = store.load().is_some();
    let status = spotify_status_text(locale, configured);

    integrations.set_spotify_configured(configured);
    integrations.set_spotify_status(status.into());
    integrations.set_spotify_client_id("".into());
    integrations.set_spotify_client_secret("".into());
}

fn handle_spotify_save(window: &MoggytWindow, store: &dyn SpotifyCredentialStore, locale: Locale) {
    let integrations = window.global::<Integrations>();
    let client_id = integrations.get_spotify_client_id();
    let client_secret = integrations.get_spotify_client_secret();
    let Some(credentials) = parse_credentials(&client_id, &client_secret) else {
        integrations.set_spotify_status(tr(locale, "Both Spotify credentials are required").into());
        return;
    };
    match store.save(&credentials) {
        Ok(()) => refresh_spotify_globals(window, store, locale),
        Err(err) => {
            tracing::warn!("failed to save Spotify credentials: {err:#}");
            integrations
                .set_spotify_status(tr(locale, "Failed to save Spotify credentials").into());
        }
    }
}

fn handle_spotify_clear(window: &MoggytWindow, store: &dyn SpotifyCredentialStore, locale: Locale) {
    let integrations = window.global::<Integrations>();
    match store.clear() {
        Ok(()) => refresh_spotify_globals(window, store, locale),
        Err(err) => {
            tracing::warn!("failed to clear Spotify credentials: {err:#}");
            integrations
                .set_spotify_status(tr(locale, "Failed to clear Spotify credentials").into());
        }
    }
}

/// Wires Spotify Web API credential saving and clearing callbacks to the Slint window.
pub(crate) fn wire_spotify_callbacks(window: &MoggytWindow, state_rc: &SharedState) {
    let locale = state_rc.borrow().locale;
    let store = KeyringSpotifyCredentialStore::moggyt();
    refresh_spotify_globals(window, &store, locale);

    let integrations = window.global::<Integrations>();

    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    integrations.on_spotify_save(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let locale = s.borrow().locale;
        let store = KeyringSpotifyCredentialStore::moggyt();
        handle_spotify_save(&win, &store, locale);
    });

    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    integrations.on_spotify_clear(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let locale = s.borrow().locale;
        let store = KeyringSpotifyCredentialStore::moggyt();
        handle_spotify_clear(&win, &store, locale);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::MemorySpotifyCredentialStore;

    #[test]
    fn test_spotify_status_text_english_and_spanish() {
        assert_eq!(spotify_status_text(Locale::English, true), "Spotify Web API: configured");
        assert_eq!(spotify_status_text(Locale::English, false), "Spotify Web API: not configured");
        assert_eq!(spotify_status_text(Locale::Spanish, true), "API web de Spotify: configurada");
        assert_eq!(
            spotify_status_text(Locale::Spanish, false),
            "API web de Spotify: no configurada"
        );
    }

    #[test]
    fn test_parse_credentials_complete_and_trimmed() {
        let parsed = parse_credentials("  my_client_id  ", "\nmy_secret\t")
            .expect("valid trimmed credentials");
        assert_eq!(parsed.client_id, "my_client_id");
        assert_eq!(parsed.client_secret, "my_secret");
        assert!(parsed.is_complete());
    }

    #[test]
    fn test_parse_credentials_incomplete_rejected() {
        assert_eq!(parse_credentials("", "secret"), None);
        assert_eq!(parse_credentials("   ", "secret"), None);
        assert_eq!(parse_credentials("id", ""), None);
        assert_eq!(parse_credentials("id", "   "), None);
        assert_eq!(parse_credentials("   ", "   "), None);
    }

    #[test]
    fn test_memory_store_roundtrip_logic() {
        let store = MemorySpotifyCredentialStore::new();
        assert_eq!(store.load(), None);

        let creds = SpotifyCredentials::new("my_client_id", "my_client_secret");
        assert!(store.save(&creds).is_ok());
        assert_eq!(store.load(), Some(creds.clone()));

        assert!(store.clear().is_ok());
        assert_eq!(store.load(), None);
    }
}
