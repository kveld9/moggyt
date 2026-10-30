//! Desktop media presence integration (MPRIS on Linux, SMTC on Windows).

use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use souvlaki::{MediaControlEvent, MediaMetadata, MediaPlayback};

use super::presence::Presence;

/// Default bounded capacity for the inbound media key action queue.
pub const ACTION_QUEUE_CAPACITY: usize = 32;

/// Inbound media key action triggered by OS media controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKeyAction {
    /// Toggle playback state between play and pause.
    Toggle,
    /// Advance to the next track.
    Next,
    /// Return to the previous track.
    Previous,
    /// Stop playback.
    Stop,
}

/// Maps an OS media control event to an application media key action.
///
/// Unmapped or unsupported events (e.g. seek, open URI, quit) evaluate to `None`.
pub fn action_for(event: MediaControlEvent) -> Option<MediaKeyAction> {
    match event {
        MediaControlEvent::Toggle => Some(MediaKeyAction::Toggle),
        MediaControlEvent::Next => Some(MediaKeyAction::Next),
        MediaControlEvent::Previous => Some(MediaKeyAction::Previous),
        MediaControlEvent::Stop => Some(MediaKeyAction::Stop),
        _ => None,
    }
}

/// Non-blocking enqueue that drops actions on full or disconnected channel.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn try_enqueue_action(sender: &SyncSender<MediaKeyAction>, action: MediaKeyAction) {
    if let Err(err) = sender.try_send(action) {
        // Deliberate drop policy: the OS callback thread must never block or panic.
        // Drops action if the queue is full or the receiver has disconnected.
        match err {
            TrySendError::Full(_) => {
                tracing::warn!("media key action queue full; dropping action");
            }
            TrySendError::Disconnected(_) => {
                tracing::debug!("media key action receiver disconnected; dropping action");
            }
        }
    }
}

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
    action_sender: SyncSender<MediaKeyAction>,
    action_receiver: Receiver<MediaKeyAction>,
}

impl DesktopPresence {
    /// Initializes media presence controls, falling back to a headless no-op on failure.
    pub fn new() -> Self {
        let (sender, receiver) = sync_channel(ACTION_QUEUE_CAPACITY);
        let controls = try_init_controls(sender.clone());
        Self {
            controls,
            last_published: None,
            last_status: None,
            action_sender: sender,
            action_receiver: receiver,
        }
    }

    /// Constructs a `DesktopPresence` instance with injected channel endpoints.
    pub fn new_with_channel(
        sender: SyncSender<MediaKeyAction>,
        receiver: Receiver<MediaKeyAction>,
    ) -> Self {
        Self {
            controls: None,
            last_published: None,
            last_status: None,
            action_sender: sender,
            action_receiver: receiver,
        }
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

    /// Returns a clone of the action sender seam.
    pub fn action_sender(&self) -> SyncSender<MediaKeyAction> {
        self.action_sender.clone()
    }

    /// Enqueues an action via the internal sender (non-blocking).
    pub fn enqueue_action(&self, action: MediaKeyAction) {
        try_enqueue_action(&self.action_sender, action);
    }

    /// Drains all pending media key actions in FIFO order.
    pub fn drain_actions(&mut self) -> Vec<MediaKeyAction> {
        self.action_receiver.try_iter().collect()
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

fn try_init_controls(sender: SyncSender<MediaKeyAction>) -> Option<souvlaki::MediaControls> {
    std::panic::catch_unwind(move || build_controls(sender)).ok().flatten()
}

fn build_controls(sender: SyncSender<MediaKeyAction>) -> Option<souvlaki::MediaControls> {
    let config =
        souvlaki::PlatformConfig { dbus_name: "moggyt", display_name: "moggyt", hwnd: None };
    let mut controls = souvlaki::MediaControls::new(config).ok()?;
    controls
        .attach(move |event| {
            if let Some(action) = action_for(event) {
                try_enqueue_action(&sender, action);
            }
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
    fn action_for_mapping_table() {
        assert_eq!(action_for(MediaControlEvent::Toggle), Some(MediaKeyAction::Toggle));
        assert_eq!(action_for(MediaControlEvent::Next), Some(MediaKeyAction::Next));
        assert_eq!(action_for(MediaControlEvent::Previous), Some(MediaKeyAction::Previous));
        assert_eq!(action_for(MediaControlEvent::Stop), Some(MediaKeyAction::Stop));

        assert_eq!(action_for(MediaControlEvent::Play), None);
        assert_eq!(action_for(MediaControlEvent::Pause), None);
        assert_eq!(action_for(MediaControlEvent::Raise), None);
        assert_eq!(action_for(MediaControlEvent::Quit), None);
        assert_eq!(
            action_for(MediaControlEvent::OpenUri("https://music.youtube.com".into())),
            None
        );
    }

    #[test]
    fn desktop_presence_queue_drain_and_drop_policy() {
        let mut presence = DesktopPresence::new();

        // Send N actions via the seam
        presence.enqueue_action(MediaKeyAction::Toggle);
        presence.enqueue_action(MediaKeyAction::Next);
        presence.enqueue_action(MediaKeyAction::Previous);
        presence.enqueue_action(MediaKeyAction::Stop);

        // Drain returns them in order
        let drained = presence.drain_actions();
        assert_eq!(
            drained,
            vec![
                MediaKeyAction::Toggle,
                MediaKeyAction::Next,
                MediaKeyAction::Previous,
                MediaKeyAction::Stop,
            ]
        );

        // Second drain is empty
        assert!(presence.drain_actions().is_empty());

        // Bounded capacity drops on full without panicking
        let (sender, receiver) = sync_channel(2);
        let mut bounded_presence = DesktopPresence::new_with_channel(sender.clone(), receiver);
        try_enqueue_action(&sender, MediaKeyAction::Toggle);
        try_enqueue_action(&sender, MediaKeyAction::Next);
        // Queue full: next action drops safely
        try_enqueue_action(&sender, MediaKeyAction::Stop);

        let bounded_drained = bounded_presence.drain_actions();
        assert_eq!(bounded_drained, vec![MediaKeyAction::Toggle, MediaKeyAction::Next]);

        // Disconnected receiver drops safely without panicking
        let (disc_sender, disc_receiver) = sync_channel(2);
        drop(disc_receiver);
        try_enqueue_action(&disc_sender, MediaKeyAction::Toggle);
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
