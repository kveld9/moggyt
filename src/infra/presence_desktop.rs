//! Desktop media presence integration (MPRIS on Linux, SMTC on Windows).

use souvlaki::{MediaMetadata, MediaPlayback};

use super::presence::Presence;

fn clean_text<'a>(value: &'a str, fallback: &'static str) -> &'a str {
    let trimmed = value.trim();
    if trimmed.is_empty() { fallback } else { trimmed }
}

/// Formats track metadata for OS media controls.
///
/// Falls back to "Unknown title" or "Unknown artist" when input is blank or whitespace.
pub fn metadata_for<'a>(artist: &'a str, title: &'a str) -> MediaMetadata<'a> {
    MediaMetadata {
        title: Some(clean_text(title, "Unknown title")),
        artist: Some(clean_text(artist, "Unknown artist")),
        album: None,
        cover_url: None,
        duration: None,
    }
}

/// Maps boolean playback state to OS media playback status.
pub fn status_for(playing: bool) -> MediaPlayback {
    if playing {
        MediaPlayback::Playing { progress: None }
    } else {
        MediaPlayback::Paused { progress: None }
    }
}

/// Native desktop media presence (MPRIS on Linux, SMTC on Windows).
pub struct DesktopPresence {
    controls: Option<souvlaki::MediaControls>,
    last_published: Option<(String, String)>,
    last_status: Option<bool>,
}

impl DesktopPresence {
    /// Initializes media presence controls, falling back to a headless no-op on failure.
    pub fn new() -> Self {
        Self { controls: try_init_controls(), last_published: None, last_status: None }
    }

    /// Returns true if native media controls were successfully attached.
    pub fn is_live(&self) -> bool {
        self.controls.is_some()
    }

    /// Sets playback status when live; always records the status seam.
    pub fn set_playing(&mut self, playing: bool) {
        self.last_status = Some(playing);
        let Some(controls) = &mut self.controls else {
            return;
        };
        let _ = controls.set_playback(status_for(playing));
    }

    /// Test seam accessor for the last published track metadata.
    pub fn last_published(&self) -> Option<(String, String)> {
        self.last_published.clone()
    }

    /// Test seam accessor for the last published playback status.
    pub fn last_status(&self) -> Option<bool> {
        self.last_status
    }
}

impl Default for DesktopPresence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence for DesktopPresence {
    fn set_track(&mut self, artist: &str, title: &str) {
        self.last_published = Some((artist.to_string(), title.to_string()));
        let Some(controls) = &mut self.controls else {
            return;
        };
        let _ = controls.set_metadata(metadata_for(artist, title));
    }

    fn clear(&mut self) {
        self.last_published = None;
        let Some(controls) = &mut self.controls else {
            return;
        };
        let _ = controls.set_metadata(MediaMetadata::default());
    }
}

fn try_init_controls() -> Option<souvlaki::MediaControls> {
    std::panic::catch_unwind(build_controls).ok().flatten()
}

fn build_controls() -> Option<souvlaki::MediaControls> {
    let config =
        souvlaki::PlatformConfig { dbus_name: "moggyt", display_name: "moggyt", hwnd: None };
    let mut controls = souvlaki::MediaControls::new(config).ok()?;
    // Media key event routing requires an egui repaint channel and is left as a pending gap;
    // this logger acts as a safe, non-blocking receiver.
    controls
        .attach(|event| {
            tracing::debug!("received media control event: {event:?}");
        })
        .ok()?;
    Some(controls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_populates_trimmed_fields_and_fallbacks() {
        let meta = metadata_for(" Radiohead ", " Creep ");
        assert_eq!(meta.title, Some("Creep"));
        assert_eq!(meta.artist, Some("Radiohead"));

        let fallback_title = metadata_for("Radiohead", "   ");
        assert_eq!(fallback_title.title, Some("Unknown title"));
        assert_eq!(fallback_title.artist, Some("Radiohead"));

        let fallback_artist = metadata_for("", "Creep");
        assert_eq!(fallback_artist.title, Some("Creep"));
        assert_eq!(fallback_artist.artist, Some("Unknown artist"));
    }

    #[test]
    fn status_for_maps_playing_boolean() {
        assert_eq!(status_for(true), MediaPlayback::Playing { progress: None });
        assert_eq!(status_for(false), MediaPlayback::Paused { progress: None });
    }

    #[test]
    fn desktop_presence_lifecycle_and_test_seams() {
        let mut presence = DesktopPresence::new();
        assert_eq!(presence.last_published(), None);
        assert_eq!(presence.last_status(), None);

        presence.set_track("Daft Punk", "Get Lucky");
        assert_eq!(
            presence.last_published(),
            Some(("Daft Punk".to_string(), "Get Lucky".to_string()))
        );

        presence.set_playing(true);
        assert_eq!(presence.last_status(), Some(true));

        presence.clear();
        assert_eq!(presence.last_published(), None);

        presence.set_playing(false);
        assert_eq!(presence.last_status(), Some(false));
    }
}
