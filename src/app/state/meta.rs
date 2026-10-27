//! In-memory and persisted metadata overrides for [`AppState`].

use crate::app::AppState;
use crate::domain::{MetadataOverride, Track};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, MetaStore, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects an alternative metadata store implementation and synchronizes state.
    pub fn set_meta_store(&mut self, store: Box<dyn MetaStore>) {
        self.meta_overrides = store.load();
        self.meta_store = store;
    }

    /// Saves a metadata override patch for `track_id` and persists to storage.
    pub fn save_meta_override(
        &mut self,
        track_id: &str,
        patch: MetadataOverride,
    ) -> anyhow::Result<()> {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            anyhow::bail!("track id cannot be blank");
        }
        self.meta_overrides.insert(trimmed.to_owned(), patch);
        if let Err(err) = self.meta_store.save(&self.meta_overrides) {
            tracing::warn!("failed to persist metadata override: {err:#}");
            return Err(err);
        }
        Ok(())
    }

    /// Clears any metadata override for `track_id` and persists clearance to storage.
    pub fn clear_meta_override(&mut self, track_id: &str) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.meta_overrides.remove(trimmed).is_some() {
            if let Err(err) = self.meta_store.save(&self.meta_overrides) {
                tracing::warn!("failed to persist metadata override clearance: {err:#}");
            }
        }
    }

    /// Resolves track title and artist for DISPLAY ONLY.
    ///
    /// Scope: now-playing current-track header and queue rows.
    /// All identity paths (likes, lyrics key + lookup args, scrobble args, desktop/discord
    /// presence, queue persistence, backup, and handoff) MUST keep canonical `track.artist`
    /// and `track.title` and must NOT route through metadata overrides.
    pub fn resolved_track_text(&self, track: &Track) -> (String, String) {
        let Some(meta) = self.meta_overrides.get(&track.id) else {
            return (track.title.clone(), track.artist.clone());
        };
        let (title, artist) = meta.resolve(track);
        (title.into_owned(), artist.into_owned())
    }

    /// Synchronizes UI edit buffers with resolved text for `track`.
    pub fn sync_meta_edit_buffers(&mut self, track: &Track) {
        if self.meta_edit_track_id.as_deref() != Some(&track.id) {
            let (title, artist) = self.resolved_track_text(track);
            self.meta_edit_title = title;
            self.meta_edit_artist = artist;
            self.meta_edit_track_id = Some(track.id.clone());
        }
    }

    /// Saves metadata override from UI edit buffers for `track`.
    ///
    /// Blank input fields mean "keep canonical for that field".
    /// If both fields are blank, any existing override is cleared.
    pub fn save_meta_override_from_input(&mut self, track: &Track) {
        let title_opt = sanitize_input_field(&self.meta_edit_title);
        let artist_opt = sanitize_input_field(&self.meta_edit_artist);
        if title_opt.is_none() && artist_opt.is_none() {
            self.clear_meta_override_and_reset(track);
            return;
        }
        let Ok(patch) = MetadataOverride::new(title_opt.as_deref(), artist_opt.as_deref()) else {
            return;
        };
        let _ = self.save_meta_override(&track.id, patch);
        let (title, artist) = self.resolved_track_text(track);
        self.meta_edit_title = title;
        self.meta_edit_artist = artist;
    }

    /// Clears metadata override for `track` and resets UI edit buffers to canonical text.
    pub fn clear_meta_override_and_reset(&mut self, track: &Track) {
        self.clear_meta_override(&track.id);
        let (title, artist) = self.resolved_track_text(track);
        self.meta_edit_title = title;
        self.meta_edit_artist = artist;
    }
}

fn sanitize_input_field(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::Track;
    use crate::infra::{DiskMetaStore, StubScrobbler};

    #[test]
    fn app_metadata_override_save_and_clear_persists_through_disk_store() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-app-meta-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut state = wired();
        let store = DiskMetaStore::new(temp_dir.clone());
        state.set_meta_store(Box::new(store));

        let track = Track::new("track-persist", "Original Title", "Original Artist");
        state.queue.push(track.clone());

        // Initial resolved text equals canonical track text
        assert_eq!(
            state.resolved_track_text(&track),
            ("Original Title".to_string(), "Original Artist".to_string())
        );

        // Save override
        let patch =
            MetadataOverride::new(Some("Custom Display Title"), None).expect("valid override");
        state.save_meta_override(&track.id, patch).expect("save override succeeds");

        // Display updates immediately
        assert_eq!(
            state.resolved_track_text(&track),
            ("Custom Display Title".to_string(), "Original Artist".to_string())
        );

        // Disk store reloaded in a fresh store instance contains the persisted override
        let fresh_store = DiskMetaStore::new(temp_dir.clone());
        let disk_overrides = fresh_store.load();
        assert_eq!(
            disk_overrides.get(&track.id).and_then(|m| m.title.as_deref()),
            Some("Custom Display Title")
        );

        // Clear override
        state.clear_meta_override(&track.id);
        assert_eq!(
            state.resolved_track_text(&track),
            ("Original Title".to_string(), "Original Artist".to_string())
        );

        // Disk store reloaded reflects the clearance
        let cleared_store = DiskMetaStore::new(temp_dir.clone());
        assert!(cleared_store.load().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn identity_scrobble_lyrics_and_presence_use_canonical_while_display_uses_resolved() {
        let mut state = wired();
        let track = Track::new_full(
            "identity-track-1",
            "Canonical Title",
            "Canonical Artist",
            Some("Album".into()),
            Some(200),
        );
        state.queue.push(track.clone());

        // Apply metadata override for title and artist
        let patch = MetadataOverride::new(Some("Overridden Title"), Some("Overridden Artist"))
            .expect("valid override");
        state.save_meta_override(&track.id, patch).expect("save succeeds");

        // Display resolves to the overridden strings
        let (disp_title, disp_artist) = state.resolved_track_text(&track);
        assert_eq!(disp_title, "Overridden Title");
        assert_eq!(disp_artist, "Overridden Artist");

        // Canonical identity check 1: play_current_quiet forwards canonical to scrobbler now_playing
        state.play_current_quiet();
        let stub =
            state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub scrobbler");
        assert_eq!(
            stub.now_playing_calls.last(),
            Some(&("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );

        // Canonical identity check 2: scrobble forwards canonical to scrobbler scrobble
        state.maybe_scrobble_with_elapsed(120);
        let stub =
            state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub scrobbler");
        assert_eq!(
            stub.scrobble_calls.last().map(|(a, t, _)| (a.as_str(), t.as_str())),
            Some(("Canonical Artist", "Canonical Title"))
        );

        // Canonical identity check 3: refresh_lyrics indexes by canonical key
        state.refresh_lyrics();
        assert_eq!(
            state.lyrics_key,
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );

        // Canonical identity check 4: presence sync uses canonical track text
        state.sync_presence();
        assert_eq!(
            state.last_presence_key,
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );
        assert_eq!(
            state.presence.last_published(),
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );
        // Discord records (details, state) = (title, artist) per `activity_parts`.
        assert_eq!(
            state.discord.last_published(),
            Some(("Canonical Title".to_string(), "Canonical Artist".to_string()))
        );

        // Canonical identity check 5: like status keys on catalog track id,
        // unaffected by the unrelated override entry (stub hits only).
        state.catalog.set_liked("stub-1", true);
        assert!(state.is_liked("stub-1"));
        assert!(!state.is_liked(&track.id));
    }

    #[test]
    fn meta_override_from_input_helpers_save_and_reset_buffers() {
        let mut state = wired();
        let track = Track::new("t-input", "Song", "Band");
        state.queue.push(track.clone());

        state.sync_meta_edit_buffers(&track);
        assert_eq!(state.meta_edit_title, "Song");
        assert_eq!(state.meta_edit_artist, "Band");

        // Edit title, leave artist blank (means keep canonical artist)
        state.meta_edit_title = "Edited Song".into();
        state.meta_edit_artist = "   ".into();
        state.save_meta_override_from_input(&track);

        let (t, a) = state.resolved_track_text(&track);
        assert_eq!(t, "Edited Song");
        assert_eq!(a, "Band");
        assert_eq!(state.meta_edit_title, "Edited Song");
        assert_eq!(state.meta_edit_artist, "Band");

        // Clear and reset
        state.clear_meta_override_and_reset(&track);
        let (t, a) = state.resolved_track_text(&track);
        assert_eq!(t, "Song");
        assert_eq!(a, "Band");
        assert_eq!(state.meta_edit_title, "Song");
        assert_eq!(state.meta_edit_artist, "Band");
    }
}
