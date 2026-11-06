//! Discord Rich Presence integration port implementation.

use discord_rich_presence::activity::{Activity, ActivityType};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use super::presence::Presence;

/// Application ID registered for "moggyt" with the Discord Developer Portal.
///
/// An ID of zero denotes an unregistered dormant state: no socket connection or IPC
/// activity will be attempted. Set to a valid Discord application client ID to activate.
pub const DISCORD_APP_ID: u64 = 1566973935109541999;

fn clean_text<'a>(value: &'a str, fallback: &'static str) -> &'a str {
    let trimmed = value.trim();
    if trimmed.is_empty() { fallback } else { trimmed }
}

fn try_connect(app_id: u64) -> Option<DiscordIpcClient> {
    if app_id == 0 {
        return None;
    }
    std::panic::catch_unwind(|| connect_ipc(app_id)).ok().flatten()
}

/// Formats track metadata into Discord activity details and state strings.
/// Returns `(details, state)` corresponding to song title and artist respectively,
/// with fallbacks to "Unknown title" and "Unknown artist" when input is blank or whitespace.
pub fn activity_parts(artist: &str, title: &str) -> (String, String) {
    (
        clean_text(title, "Unknown title").to_string(),
        clean_text(artist, "Unknown artist").to_string(),
    )
}

/// Discord Rich Presence adapter.
///
/// Remains zero-cost and dormant when initialized with an unregistered application ID (0).
pub struct DiscordPresence {
    app_id: u64,
    client: Option<DiscordIpcClient>,
    last_published: Option<(String, String)>,
}

impl DiscordPresence {
    /// Initializes Discord Rich Presence using [`DISCORD_APP_ID`].
    ///
    /// Dormant when [`DISCORD_APP_ID`] is zero; otherwise safely attempts IPC connection.
    pub fn new() -> Self {
        Self::with_app_id(DISCORD_APP_ID)
    }

    /// Initializes Discord Rich Presence in a dormant state (app ID 0) that never touches IPC.
    pub fn dormant() -> Self {
        Self::with_app_id(0)
    }

    /// Initializes Discord Rich Presence for a specific application ID.
    ///
    /// Passing 0 remains dormant without touching IPC. Non-zero IDs attempt connection
    /// guarded by panic catch, falling back to dormant on any failure.
    pub fn with_app_id(app_id: u64) -> Self {
        Self { app_id, client: try_connect(app_id), last_published: None }
    }

    /// Returns true if connected to the Discord IPC socket.
    pub fn is_live(&self) -> bool {
        self.client.is_some()
    }

    /// Test seam accessor for the last published activity parts (details, state).
    pub fn last_published(&self) -> Option<(String, String)> {
        self.last_published.clone()
    }

    /// Clears active status, closes any active IPC connection, and resets presence state.
    // NOTE(clippy): tracing macro expansion inflates cognitive complexity (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub fn shutdown(&mut self) {
        if let Some(mut client) = self.client.take() {
            if let Err(err) = client.clear_activity() {
                tracing::debug!("failed to clear discord activity on shutdown: {err:#}");
            }
            if let Err(err) = client.close() {
                tracing::debug!("failed to close discord ipc client: {err:#}");
            }
        }
        self.last_published = None;
    }
}

impl Default for DiscordPresence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence for DiscordPresence {
    /// Publishes track metadata to Discord Rich Presence when live; always records the test seam.
    ///
    /// On IPC error, logs a warning and drops the client so subsequent tracks attempt reconnect.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn set_track(&mut self, artist: &str, title: &str) {
        let (details, state) = activity_parts(artist, title);
        self.last_published = Some((details.clone(), state.clone()));
        if self.client.is_none() && self.app_id != 0 {
            self.client = try_connect(self.app_id);
        }
        let Some(client) = &mut self.client else {
            return;
        };
        let activity =
            Activity::new().activity_type(ActivityType::Listening).details(&details).state(&state);
        if let Err(err) = client.set_activity(activity) {
            tracing::warn!("failed to set discord activity: {err:#}");
            self.client = None;
        }
    }

    /// Clears the active Discord Rich Presence activity.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn clear(&mut self) {
        self.last_published = None;
        let Some(client) = &mut self.client else {
            return;
        };
        if let Err(err) = client.clear_activity() {
            tracing::warn!("failed to clear discord activity: {err:#}");
        }
    }
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn connect_ipc(app_id: u64) -> Option<DiscordIpcClient> {
    let mut client = DiscordIpcClient::new(app_id.to_string());
    if let Err(err) = client.connect() {
        tracing::debug!("failed to connect to discord ipc socket: {err:#}");
        return None;
    }
    Some(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_parts_populates_trimmed_fields_and_fallbacks() {
        let (details, state) = activity_parts(" Radiohead ", " Creep ");
        assert_eq!(details, "Creep");
        assert_eq!(state, "Radiohead");

        let (fallback_title, state) = activity_parts("Radiohead", "   ");
        assert_eq!(fallback_title, "Unknown title");
        assert_eq!(state, "Radiohead");

        let (details, fallback_artist) = activity_parts("", "Creep");
        assert_eq!(details, "Creep");
        assert_eq!(fallback_artist, "Unknown artist");
    }

    #[test]
    fn discord_presence_lifecycle_and_test_seams() {
        let mut presence = DiscordPresence::dormant();
        assert!(!presence.is_live());
        assert_eq!(presence.last_published(), None);

        presence.set_track("Daft Punk", "Get Lucky");
        assert_eq!(
            presence.last_published(),
            Some(("Get Lucky".to_string(), "Daft Punk".to_string()))
        );

        presence.clear();
        assert_eq!(presence.last_published(), None);
    }

    #[test]
    fn discord_presence_with_app_id_zero_is_dormant() {
        let p0 = DiscordPresence::with_app_id(0);
        assert!(!p0.is_live());

        let p_dormant = DiscordPresence::dormant();
        assert!(!p_dormant.is_live());
    }

    #[test]
    fn discord_presence_shutdown_clears_last_published() {
        let mut presence = DiscordPresence::dormant();
        presence.set_track("Daft Punk", "Get Lucky");
        assert!(presence.last_published().is_some());

        presence.shutdown();
        assert_eq!(presence.last_published(), None);
        assert!(!presence.is_live());
    }

    #[test]
    fn discord_app_id_is_configured() {
        assert_ne!(DISCORD_APP_ID, 0);
        assert_eq!(DISCORD_APP_ID, 1566973935109541999);
    }
}
