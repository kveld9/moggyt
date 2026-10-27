//! Desktop MPRIS/SMTC, Discord Rich Presence, and system tray integration for [`AppState`].

use crate::app::AppState;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, DesktopPresence, DiscordPresence, LyricsSource,
    MediaKeyAction, Presence, SessionStore, TrayAction, TrayHandle,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Attaches an initialized system tray handle.
    pub fn set_tray(&mut self, handle: TrayHandle) {
        self.tray = Some(handle);
    }

    /// Attaches an initialized desktop media presence handle.
    pub fn set_presence(&mut self, presence: DesktopPresence) {
        self.presence = presence;
    }

    /// Attaches an initialized Discord Rich Presence handle.
    pub fn set_discord(&mut self, discord: DiscordPresence) {
        self.discord = discord;
    }

    /// Dispatches a single tray action, returning true if quit was requested.
    fn handle_action(&mut self, action: TrayAction) -> bool {
        match action {
            TrayAction::TogglePlay => self.toggle_play(),
            TrayAction::Next => self.next_track(),
            TrayAction::Previous => self.prev_track(),
            TrayAction::Show => {}
            TrayAction::Quit => return true,
        }
        false
    }

    /// Applies a batch of tray actions to application state.
    pub fn apply_actions(&mut self, actions: &[TrayAction]) -> bool {
        let mut quit_requested = false;
        for action in actions {
            if self.handle_action(*action) {
                quit_requested = true;
            }
        }
        quit_requested
    }

    /// Drains pending system tray events and applies them to state.
    ///
    /// Returns true when a Quit action was received.
    pub fn drain_tray(&mut self) -> bool {
        let Some(tray) = &self.tray else {
            return false;
        };
        let actions = tray.drain();
        self.apply_actions(&actions)
    }

    /// Dispatches a single media key action to application state.
    fn handle_media_key(&mut self, action: MediaKeyAction) {
        match action {
            MediaKeyAction::Toggle => self.toggle_play(),
            MediaKeyAction::Next => self.next_track(),
            MediaKeyAction::Previous => self.prev_track(),
            MediaKeyAction::Stop => self.stop_playback(),
        }
    }

    /// Applies a batch of media key actions to application state.
    fn apply_media_keys(&mut self, actions: &[MediaKeyAction]) {
        for action in actions {
            self.handle_media_key(*action);
        }
    }

    /// Drains pending inbound media key actions from desktop presence and applies them.
    fn drain_media_keys(&mut self) {
        let actions = self.presence.drain_actions();
        self.apply_media_keys(&actions);
    }

    /// Synchronizes current playback state and track metadata with presence adapters.
    ///
    /// Drains inbound media key actions first, then fans out active track metadata or queue
    /// clearance to both desktop media presence (MPRIS on Linux, SMTC on Windows) and Discord Rich Presence.
    pub fn sync_presence(&mut self) {
        self.drain_media_keys();
        let is_playing = self.audio.is_playing();
        let track_info = self.queue.current().map(|t| (t.artist.clone(), t.title.clone()));
        match track_info {
            Some((artist, title)) => self.sync_active_track(&artist, &title, is_playing),
            None => self.sync_empty_track(),
        }
    }

    fn sync_active_track(&mut self, artist: &str, title: &str, is_playing: bool) {
        let key = (artist.to_string(), title.to_string());
        if self.last_presence_key.as_ref() != Some(&key) {
            self.presence.set_track(artist, title);
            self.discord.set_track(artist, title);
            self.last_presence_key = Some(key);
        }
        self.presence.set_playing(is_playing);
    }

    fn sync_empty_track(&mut self) {
        if self.last_presence_key.is_some() || self.presence.last_published().is_some() {
            self.presence.clear();
            self.discord.clear();
            self.last_presence_key = None;
        }
        self.presence.set_playing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::Track;
    use crate::infra::{MediaKeyAction, TrayAction};

    #[test]
    fn drain_tray_with_none_returns_false_and_preserves_state() {
        let mut state = wired();
        assert!(state.tray.is_none());
        let quit = state.drain_tray();
        assert!(!quit);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn apply_actions_empty_and_show_return_false() {
        let mut state = wired();
        assert!(!state.apply_actions(&[]));
        assert!(!state.apply_actions(&[TrayAction::Show]));
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn apply_actions_toggle_next_prev_and_quit() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        // Toggle starts playback of head track
        let quit = state.apply_actions(&[TrayAction::TogglePlay]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Next advances to sunrise and plays
        let quit = state.apply_actions(&[TrayAction::Next]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        // Previous retreats to midnight and plays
        let quit = state.apply_actions(&[TrayAction::Previous]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Quit returns true
        assert!(state.apply_actions(&[TrayAction::Quit]));

        // Compound batch: TogglePlay then Quit toggles (pauses) and returns true
        let quit = state.apply_actions(&[TrayAction::TogglePlay, TrayAction::Quit]);
        assert!(quit);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn inbound_media_keys_dispatch_via_presence_queue() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);
        assert!(!state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Queued Toggle flips playback state via existing fakes (starts playing head)
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Queued Next advances the queue head and plays
        state.presence.enqueue_action(MediaKeyAction::Next);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        // Queued Toggle flips playback state back to paused
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Queued Stop on paused playback remains paused
        state.presence.enqueue_action(MediaKeyAction::Stop);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Resume playback with Toggle, then Stop pauses
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(state.audio.is_playing());

        state.presence.enqueue_action(MediaKeyAction::Stop);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Queued Previous retreats the queue head and plays
        state.presence.enqueue_action(MediaKeyAction::Previous);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
    }

    #[test]
    fn sync_presence_with_empty_queue_clears_and_tracks_status() {
        let mut state = wired();
        state.sync_presence();
        assert_eq!(state.presence.last_published(), None);
        assert_eq!(state.presence.last_status(), Some(false));
    }

    #[test]
    fn sync_presence_with_queued_track_publishes_and_deduplicates_key() {
        let mut state = wired();
        let track = Track {
            id: "vid-1".into(),
            title: "Paranoid Android".into(),
            artist: "Radiohead".into(),
            album: None,
            duration_secs: Some(383),
            artwork: None,
        };
        state.queue.push(track);
        state.audio.play("vid-1");

        state.sync_presence();
        assert_eq!(
            state.presence.last_published(),
            Some(("Radiohead".into(), "Paranoid Android".into()))
        );
        assert_eq!(state.presence.last_status(), Some(true));
        assert_eq!(state.last_presence_key, Some(("Radiohead".into(), "Paranoid Android".into())));

        // Pause: set_track skipped because key unchanged, but set_playing updated to false
        state.audio.pause();
        state.sync_presence();
        assert_eq!(state.presence.last_status(), Some(false));
        assert_eq!(state.last_presence_key, Some(("Radiohead".into(), "Paranoid Android".into())));

        // Clear queue: sync clears presence metadata and resets key
        state.queue.clear();
        state.sync_presence();
        assert_eq!(state.presence.last_published(), None);
        assert_eq!(state.presence.last_status(), Some(false));
        assert_eq!(state.last_presence_key, None);
    }

    #[test]
    fn sync_presence_fans_out_to_discord_presence_and_clears_on_empty() {
        let mut state = wired();
        assert_eq!(state.discord.last_published(), None);

        let track = Track {
            id: "vid-1".into(),
            title: "Paranoid Android".into(),
            artist: "Radiohead".into(),
            album: None,
            duration_secs: Some(383),
            artwork: None,
        };
        state.queue.push(track);
        state.audio.play("vid-1");

        state.sync_presence();
        assert_eq!(
            state.discord.last_published(),
            Some(("Paranoid Android".into(), "Radiohead".into()))
        );

        state.queue.clear();
        state.sync_presence();
        assert_eq!(state.discord.last_published(), None);
    }
}
