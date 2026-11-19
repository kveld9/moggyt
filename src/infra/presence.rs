//! Media presence integration port and no-op stub (MPRIS/SMTC in production).

/// Track metadata published to OS and Discord presence surfaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceTrack {
    /// Artist display name, passed through unchanged.
    pub artist: String,
    /// Track title, passed through unchanged.
    pub title: String,
    /// Remote cover art URL, normalized to `None` when missing or blank.
    pub artwork: Option<String>,
    /// Track length in seconds used for `mpris:length`, normalized to `None` when missing or zero.
    pub duration_secs: Option<u64>,
}

impl PresenceTrack {
    /// Builds presence metadata, trimming the artwork hint and dropping blank values.
    pub fn new(artist: &str, title: &str, artwork: Option<&str>) -> Self {
        let artwork = artwork.map(str::trim).filter(|s| !s.is_empty()).map(ToString::to_string);
        Self { artist: artist.to_string(), title: title.to_string(), artwork, duration_secs: None }
    }

    /// Sets the track length in seconds, normalizing zero to `None`.
    pub fn with_duration(mut self, duration_secs: Option<u32>) -> Self {
        self.duration_secs = duration_secs.filter(|&secs| secs > 0).map(u64::from);
        self
    }
}

pub trait Presence {
    fn set_track(&mut self, track: &PresenceTrack);
    /// Reports the current playback position; consumers without progress support ignore it.
    fn set_position(&mut self, position_secs: u64);
    fn clear(&mut self);
}

#[derive(Debug, Default)]
pub struct NoopPresence;

impl NoopPresence {
    pub fn new() -> Self {
        Self
    }
}

impl Presence for NoopPresence {
    fn set_track(&mut self, _track: &PresenceTrack) {}
    fn set_position(&mut self, _position_secs: u64) {}
    fn clear(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_track_new_normalizes_artwork() {
        let track = PresenceTrack::new("Artist", "Title", Some(" https://example.com/art.jpg  "));
        assert_eq!(track.artist, "Artist");
        assert_eq!(track.title, "Title");
        assert_eq!(track.artwork.as_deref(), Some("https://example.com/art.jpg"));

        let empty = PresenceTrack::new("Artist", "Title", Some("   "));
        assert_eq!(empty.artwork, None);

        let none = PresenceTrack::new("Artist", "Title", None);
        assert_eq!(none.artwork, None);
    }

    #[test]
    fn presence_track_with_duration_normalizes_zero_and_keeps_positive() {
        let base = PresenceTrack::new("Artist", "Title", None);
        assert_eq!(base.duration_secs, None);

        let positive = base.clone().with_duration(Some(180));
        assert_eq!(positive.duration_secs, Some(180));

        let zero = base.clone().with_duration(Some(0));
        assert_eq!(zero.duration_secs, None);

        let none = base.with_duration(None);
        assert_eq!(none.duration_secs, None);
    }

    #[test]
    fn noop_presence_calls_do_not_panic() {
        let mut presence = NoopPresence::new();
        let track = PresenceTrack::new("Artist", "Title", Some("https://example.com/art.jpg"))
            .with_duration(Some(210));
        presence.set_track(&track);
        presence.set_position(42);
        presence.clear();
    }
}
