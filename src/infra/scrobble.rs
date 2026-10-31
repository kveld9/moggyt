//! Scrobbler and credential store ports, pure scrobble policy, and adapter re-exports.
//!
//! Exposes the [`Scrobbler`] trait for playback progress reporting,
//! credential persistence via [`CredentialStore`], the pure [`should_scrobble`]
//! eligibility policy, and re-exports concrete adapters for backward compatibility.

pub use super::scrobble_credentials::{KeyringCredentialStore, MemoryCredentialStore};
pub use super::scrobble_lastfm::{
    LastFmClient, LastFmCredentials, api_signature, client_from_credentials,
};
pub use super::scrobble_stub::StubScrobbler;

/// Port for publishing playback status and recording scrobbles.
pub trait Scrobbler: Send {
    /// Notifies the scrobbler that a new track has begun playing.
    fn now_playing(&mut self, artist: &str, title: &str);

    /// Submits a completed scrobble record for a track.
    fn scrobble(&mut self, artist: &str, title: &str, timestamp_secs: u64);
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

/// Port for persisting and retrieving Last.fm credentials.
pub trait CredentialStore: Send + Sync {
    /// Saves Last.fm credentials, or clears them when incomplete.
    fn save_lastfm(&mut self, creds: &LastFmCredentials);
    /// Loads persisted credentials if all three fields are present and non-empty.
    fn load_lastfm(&self) -> Option<LastFmCredentials>;
    /// Clears any persisted Last.fm credentials.
    fn clear_lastfm(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
