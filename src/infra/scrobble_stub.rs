//! In-memory recording stub for the scrobbler port.

use super::scrobble::Scrobbler;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
