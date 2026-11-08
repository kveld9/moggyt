//! Last.fm desktop browser authentication flow, session polling, and UI callback wiring.
//!
//! Hexagonal invariant: pure status derivation and poll step progression separated from
//! background network requests, browser process launching, and OS keyring storage.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use slint::ComponentHandle;

use crate::i18n::{Locale, tr};
use crate::infra::{
    CredentialStore, KeyringCredentialStore, LastFmAppKeys, LastFmClient, LastFmCredentials,
    LastFmSession, Scrobbler, SessionPoll, StubScrobbler, authorize_url, open_url, request_session,
    request_token, resolve_app_keys,
};
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::{Integrations, MoggytWindow};

pub(crate) const LASTFM_POLL_INTERVAL: Duration = Duration::from_secs(3);
pub(crate) const LASTFM_POLL_MAX_ATTEMPTS: u32 = 60;

/// Pure decision step for Last.fm session polling progression.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PollStep {
    Connected(LastFmSession),
    Wait,
    GiveUp(String),
}

/// Evaluates a session poll response at a given attempt index into a deterministic step.
pub(crate) fn poll_step(poll: SessionPoll, attempt: u32) -> PollStep {
    match poll {
        SessionPoll::Ready(session) => PollStep::Connected(session),
        SessionPoll::Pending => {
            if attempt + 1 >= LASTFM_POLL_MAX_ATTEMPTS {
                PollStep::GiveUp("authorization timed out".to_string())
            } else {
                PollStep::Wait
            }
        }
        SessionPoll::Failed(m) => PollStep::GiveUp(m),
    }
}

/// Computes localized status text for Last.fm integration display.
pub(crate) fn lastfm_status_text(locale: Locale, is_available: bool, username: &str) -> String {
    if !is_available {
        return tr(locale, "Last.fm is not available in this build").to_string();
    }
    if username.is_empty() {
        return tr(locale, "Not connected").to_string();
    }
    format!("{} {username}", tr(locale, "Connected as"))
}

fn replace_scrobbler(
    scrobbler_mutex: &Arc<Mutex<Box<dyn Scrobbler>>>,
    new_scrobbler: Box<dyn Scrobbler>,
) {
    match scrobbler_mutex.lock() {
        Ok(mut guard) => *guard = new_scrobbler,
        Err(poisoned) => *poisoned.into_inner() = new_scrobbler,
    }
}

impl SlintAppState {
    /// Persists session credentials, updates settings, and swaps the active live scrobbler.
    // NOTE(clippy): tracing macro expansion inflates cognitive complexity.
    #[allow(clippy::cognitive_complexity)]
    pub(crate) fn apply_lastfm_session(
        &mut self,
        store: &mut dyn CredentialStore,
        keys: &LastFmAppKeys,
        session: &LastFmSession,
    ) {
        store.save_lastfm(&LastFmCredentials::new(
            &keys.api_key,
            &keys.secret,
            &session.session_key,
        ));
        let mut settings = self.settings_store.load();
        settings.lastfm_enabled = true;
        settings.lastfm_username = session.username.clone();
        if let Err(err) = self.settings_store.save(&settings) {
            tracing::warn!("failed to persist Last.fm settings: {err:#}");
        }
        let client = LastFmClient::from_app_keys(keys, &session.session_key);
        replace_scrobbler(&self.scrobbler, Box::new(client));
    }

    /// Clears persisted credentials, resets settings, and replaces scrobbler with a stub.
    // NOTE(clippy): tracing macro expansion inflates cognitive complexity.
    #[allow(clippy::cognitive_complexity)]
    pub(crate) fn disconnect_lastfm(&mut self, store: &mut dyn CredentialStore) {
        store.clear_lastfm();
        let mut settings = self.settings_store.load();
        settings.lastfm_enabled = false;
        settings.lastfm_username.clear();
        if let Err(err) = self.settings_store.save(&settings) {
            tracing::warn!("failed to persist Last.fm settings: {err:#}");
        }
        replace_scrobbler(&self.scrobbler, Box::new(StubScrobbler::new()));
    }
}

/// Refreshes all Last.fm integration properties in the Slint window.
pub(crate) fn refresh_lastfm_globals(window: &MoggytWindow, state: &SlintAppState) {
    let integrations = window.global::<Integrations>();
    let settings = state.settings_store.load();
    let is_available =
        resolve_app_keys(&settings.lastfm_api_key, &settings.lastfm_api_secret).is_some();
    let is_connected = !settings.lastfm_username.is_empty() && settings.lastfm_enabled;
    let status = lastfm_status_text(state.locale, is_available, &settings.lastfm_username);

    integrations.set_lastfm_available(is_available);
    integrations.set_lastfm_connected(is_connected);
    integrations.set_lastfm_busy(false);
    integrations.set_lastfm_status(status.into());
}

fn poll_session_loop(keys: &LastFmAppKeys, token: &str) -> Result<LastFmSession, String> {
    for attempt in 0..LASTFM_POLL_MAX_ATTEMPTS {
        std::thread::sleep(LASTFM_POLL_INTERVAL);
        let poll = request_session(keys, token);
        match poll_step(poll, attempt) {
            PollStep::Connected(session) => return Ok(session),
            PollStep::Wait => {}
            PollStep::GiveUp(err) => return Err(err),
        }
    }
    Err("authorization timed out".to_string())
}

fn run_connect_flow(keys: &LastFmAppKeys) -> Result<LastFmSession, String> {
    let token = request_token(keys).map_err(|e| format!("failed to request token: {e:#}"))?;
    let auth_url = authorize_url(&keys.api_key, &token);
    open_url(&auth_url).map_err(|e| format!("failed to open browser: {e:#}"))?;
    poll_session_loop(keys, &token)
}

// NOTE(clippy): tracing macro expansion inflates cognitive complexity.
#[allow(clippy::cognitive_complexity)]
fn handle_connect_error(window: &MoggytWindow, locale: Locale, msg: &str) {
    tracing::warn!("Last.fm connection failed: {msg}");
    let integrations = window.global::<Integrations>();
    integrations.set_lastfm_busy(false);
    integrations.set_lastfm_status(tr(locale, "Last.fm connection failed").into());
}

fn handle_connect_success(window: &MoggytWindow, keys: &LastFmAppKeys, session: &LastFmSession) {
    let applied = crate::slint_shell::window::with_ui_state(|st| {
        let mut store = KeyringCredentialStore::moggyt();
        st.apply_lastfm_session(&mut store, keys, session);
        refresh_lastfm_globals(window, st);
    });
    if applied.is_none() {
        window.global::<Integrations>().set_lastfm_busy(false);
    }
}

fn spawn_lastfm_connect_thread(
    weak: slint::Weak<MoggytWindow>,
    keys: LastFmAppKeys,
    locale: Locale,
) {
    std::thread::Builder::new()
        .name("lastfm-auth".to_string())
        .spawn(move || {
            let result = run_connect_flow(&keys);
            let _ = dispatch_to_ui(move || {
                let Some(win) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(session) => handle_connect_success(&win, &keys, &session),
                    Err(msg) => handle_connect_error(&win, locale, &msg),
                }
            });
        })
        .expect("spawn lastfm connect thread");
}

/// Wires Last.fm authentication and disconnect callbacks to the Slint window.
pub(crate) fn wire_lastfm_callbacks(window: &MoggytWindow, state_rc: &SharedState) {
    refresh_lastfm_globals(window, &state_rc.borrow());

    let integrations = window.global::<Integrations>();
    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    integrations.on_lastfm_connect(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let integrations = win.global::<Integrations>();
        if integrations.get_lastfm_busy() {
            return;
        }
        let settings = s.borrow().settings_store.load();
        let Some(keys) = resolve_app_keys(&settings.lastfm_api_key, &settings.lastfm_api_secret)
        else {
            return;
        };
        let locale = s.borrow().locale;
        integrations.set_lastfm_busy(true);
        integrations
            .set_lastfm_status(tr(locale, "Waiting for authorization in your browser").into());
        spawn_lastfm_connect_thread(win.as_weak(), keys, locale);
    });

    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    integrations.on_lastfm_disconnect(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let mut st = s.borrow_mut();
        let mut store = KeyringCredentialStore::moggyt();
        st.disconnect_lastfm(&mut store);
        refresh_lastfm_globals(&win, &st);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::MemoryCredentialStore;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_poll_step_ready_connected() {
        let session = LastFmSession { username: "user".to_string(), session_key: "sk".to_string() };
        let step = poll_step(SessionPoll::Ready(session.clone()), 0);
        assert_eq!(step, PollStep::Connected(session));
    }

    #[test]
    fn test_poll_step_pending_wait() {
        let step = poll_step(SessionPoll::Pending, 0);
        assert_eq!(step, PollStep::Wait);

        let step_middle = poll_step(SessionPoll::Pending, 58);
        assert_eq!(step_middle, PollStep::Wait);
    }

    #[test]
    fn test_poll_step_pending_timeout_on_last_attempt() {
        let step = poll_step(SessionPoll::Pending, LASTFM_POLL_MAX_ATTEMPTS - 1);
        assert_eq!(step, PollStep::GiveUp("authorization timed out".to_string()));
    }

    #[test]
    fn test_poll_step_failed_gives_up() {
        let step = poll_step(SessionPoll::Failed("network error".to_string()), 0);
        assert_eq!(step, PollStep::GiveUp("network error".to_string()));
    }

    #[test]
    fn test_lastfm_status_text() {
        assert_eq!(
            lastfm_status_text(Locale::English, false, "user"),
            "Last.fm is not available in this build"
        );
        assert_eq!(lastfm_status_text(Locale::English, true, ""), "Not connected");
        assert_eq!(
            lastfm_status_text(Locale::English, true, "moggy_fan"),
            "Connected as moggy_fan"
        );
    }

    #[test]
    fn test_apply_lastfm_session_saves_credentials_and_updates_settings() {
        let mut state = test_wired();
        let mut store = MemoryCredentialStore::new();
        let keys =
            LastFmAppKeys { api_key: "test_key".to_string(), secret: "test_secret".to_string() };
        let session = LastFmSession {
            username: "moggy_user".to_string(),
            session_key: "test_sk".to_string(),
        };

        state.apply_lastfm_session(&mut store, &keys, &session);

        let saved = store.load_lastfm().expect("creds must be saved");
        assert_eq!(saved.api_key, "test_key");
        assert_eq!(saved.secret, "test_secret");
        assert_eq!(saved.session_key, "test_sk");

        let settings = state.settings_store.load();
        assert!(settings.lastfm_enabled);
        assert_eq!(settings.lastfm_username, "moggy_user");
    }

    #[test]
    fn test_disconnect_lastfm_clears_store_and_settings() {
        let mut state = test_wired();
        let mut store = MemoryCredentialStore::new();
        let keys =
            LastFmAppKeys { api_key: "test_key".to_string(), secret: "test_secret".to_string() };
        let session = LastFmSession {
            username: "moggy_user".to_string(),
            session_key: "test_sk".to_string(),
        };

        state.apply_lastfm_session(&mut store, &keys, &session);
        assert!(store.load_lastfm().is_some());
        assert!(state.settings_store.load().lastfm_enabled);

        state.disconnect_lastfm(&mut store);

        assert!(store.load_lastfm().is_none());
        let settings = state.settings_store.load();
        assert!(!settings.lastfm_enabled);
        assert!(settings.lastfm_username.is_empty());
    }

    #[test]
    fn test_resolve_app_keys_from_settings_state() {
        let state = test_wired();
        let mut settings = state.settings_store.load();
        settings.lastfm_api_key = "my_key".to_string();
        settings.lastfm_api_secret = "my_sec".to_string();
        state.settings_store.save(&settings).unwrap();

        let loaded = state.settings_store.load();
        let keys = resolve_app_keys(&loaded.lastfm_api_key, &loaded.lastfm_api_secret)
            .expect("keys must resolve");
        assert_eq!(keys.api_key, "my_key");
        assert_eq!(keys.secret, "my_sec");
    }
}
