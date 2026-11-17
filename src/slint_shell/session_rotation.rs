//! Background rotation of the stored session tokens.

use std::time::Duration;

use crate::infra::{KeyringSessionStore, SessionStore, rotate_cookies};
use crate::slint_shell::state::SharedCatalog;

/// How often the stored session tokens are refreshed while the app runs.
const ROTATE_INTERVAL: Duration = Duration::from_secs(540);

/// Keeps the stored session fresh in the background.
///
/// The rotation endpoint only issues new tokens while the session is still
/// valid, so a stale session is left untouched and reported by the library
/// fetch instead of being silently rewritten.
pub fn spawn_session_rotation(catalog: SharedCatalog) {
    let _ = std::thread::Builder::new().name("moggyt-session-rotate".into()).spawn(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        loop {
            rotate_once(&client, &catalog);
            std::thread::sleep(ROTATE_INTERVAL);
        }
    });
}

fn rotate_once(client: &reqwest::blocking::Client, catalog: &SharedCatalog) {
    let mut store = KeyringSessionStore::moggyt();
    let Some(cookies) = store.load() else {
        return;
    };
    let Ok(rotated) = rotate_cookies(client, &cookies) else {
        return;
    };
    let Some(fresh) = changed_cookies(&cookies, &rotated) else {
        return;
    };
    store.save(fresh);
    if let Ok(mut catalog) = catalog.lock() {
        catalog.set_session_cookies(Some(fresh));
    }
}

/// Returns the rotated header only when it actually changed.
fn changed_cookies<'a>(current: &'a str, rotated: &'a str) -> Option<&'a str> {
    (rotated != current).then_some(rotated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_rotation_is_not_persisted() {
        assert_eq!(changed_cookies("SID=a", "SID=a"), None);
    }

    #[test]
    fn rotated_tokens_are_persisted() {
        assert_eq!(
            changed_cookies("SID=a", "SID=a; __Secure-1PSIDTS=b"),
            Some("SID=a; __Secure-1PSIDTS=b")
        );
    }
}
