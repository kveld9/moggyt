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
}

impl PresenceTrack {
    /// Builds presence metadata, trimming the artwork hint and dropping blank values.
    pub fn new(artist: &str, title: &str, artwork: Option<&str>) -> Self {
        let artwork = artwork.map(str::trim).filter(|s| !s.is_empty()).map(ToString::to_string);
        Self { artist: artist.to_string(), title: title.to_string(), artwork }
    }
}

pub trait Presence {
    fn set_track(&mut self, track: &PresenceTrack);
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
    fn noop_presence_calls_do_not_panic() {
        let mut presence = NoopPresence::new();
        let track = PresenceTrack::new("Artist", "Title", Some("https://example.com/art.jpg"));
        presence.set_track(&track);
        presence.clear();
    }
}
