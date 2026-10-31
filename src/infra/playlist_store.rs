//! Local playlist storage port and in-memory adapter.
//!
//! Provides the [`PlaylistStore`] port for persisting imported playlists
//! and [`MemoryPlaylistStore`] for in-memory testing and headless execution.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::domain::{LocalPlaylist, is_local_playlist_id};

/// Port for reading and persisting imported local playlists.
pub trait PlaylistStore: Send + Sync {
    /// Returns every stored playlist, ordered by title then id.
    fn list(&self) -> Vec<LocalPlaylist>;
    /// Returns one playlist by id.
    fn get(&self, id: &str) -> Option<LocalPlaylist>;
    /// Inserts or replaces a playlist by id.
    fn save(&self, playlist: &LocalPlaylist) -> anyhow::Result<()>;
    /// Removes a playlist by id; deleting an unknown id is not an error.
    fn delete(&self, id: &str) -> anyhow::Result<()>;
}

/// Sorts playlists by title ascending, then by id ascending.
pub(crate) fn sort_playlists(playlists: &mut [LocalPlaylist]) {
    playlists.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
}

/// In-memory playlist store for tests and headless runs.
#[derive(Debug, Clone)]
pub struct MemoryPlaylistStore {
    inner: Arc<Mutex<BTreeMap<String, LocalPlaylist>>>,
}

impl Default for MemoryPlaylistStore {
    fn default() -> Self {
        Self { inner: Arc::new(Mutex::new(BTreeMap::new())) }
    }
}

impl MemoryPlaylistStore {
    /// Creates a new empty in-memory playlist store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PlaylistStore for MemoryPlaylistStore {
    fn list(&self) -> Vec<LocalPlaylist> {
        let mut playlists: Vec<LocalPlaylist> =
            self.inner.lock().map(|guard| guard.values().cloned().collect()).unwrap_or_default();
        sort_playlists(&mut playlists);
        playlists
    }

    fn get(&self, id: &str) -> Option<LocalPlaylist> {
        self.inner.lock().ok()?.get(id).cloned()
    }

    fn save(&self, playlist: &LocalPlaylist) -> anyhow::Result<()> {
        if !is_local_playlist_id(&playlist.id) {
            anyhow::bail!("invalid local playlist id '{}'", playlist.id);
        }
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        guard.insert(playlist.id.clone(), playlist.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        guard.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchConfidence};

    #[test]
    fn test_memory_store_save_get_round_trip() {
        let store = MemoryPlaylistStore::new();
        assert_eq!(store.get("local:p1"), None);
        assert!(store.list().is_empty());

        let mut playlist = LocalPlaylist::new("local:p1", "Rock Classics").with_author("Curator");
        playlist.tracks.push(LocalTrack::new("v1", "Song 1", "Artist 1", MatchConfidence::High));

        store.save(&playlist).expect("save succeeds");
        assert_eq!(store.get("local:p1"), Some(playlist.clone()));
        assert_eq!(store.list(), vec![playlist]);
    }

    #[test]
    fn test_memory_store_upsert_replaces_entry() {
        let store = MemoryPlaylistStore::new();
        let p1 = LocalPlaylist::new("local:p1", "Initial");
        let p2 = LocalPlaylist::new("local:p1", "Updated");

        store.save(&p1).expect("first save succeeds");
        store.save(&p2).expect("second save succeeds");

        assert_eq!(store.list().len(), 1);
        assert_eq!(store.get("local:p1").unwrap().title, "Updated");
    }

    #[test]
    fn test_memory_store_delete_removes_entry_and_unknown_id_is_noop() {
        let store = MemoryPlaylistStore::new();
        let p1 = LocalPlaylist::new("local:p1", "My Playlist");

        store.save(&p1).expect("save succeeds");
        store.delete("local:unknown").expect("unknown delete is ok");
        assert_eq!(store.list().len(), 1);

        store.delete("local:p1").expect("delete succeeds");
        assert_eq!(store.get("local:p1"), None);
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_memory_store_list_orders_by_title_then_id() {
        let store = MemoryPlaylistStore::new();
        let p3 = LocalPlaylist::new("local:3", "Zeta");
        let p2 = LocalPlaylist::new("local:2", "Alpha");
        let p1 = LocalPlaylist::new("local:1", "Alpha");
        let p0 = LocalPlaylist::new("local:0", "Beta");

        store.save(&p3).unwrap();
        store.save(&p2).unwrap();
        store.save(&p1).unwrap();
        store.save(&p0).unwrap();

        let list = store.list();
        let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["local:1", "local:2", "local:0", "local:3"]);
    }

    #[test]
    fn test_memory_store_save_rejects_non_local_id() {
        let store = MemoryPlaylistStore::new();

        let bad_spotify = LocalPlaylist::new("spotify:12345", "Spotify Playlist");
        let err = store.save(&bad_spotify).unwrap_err();
        assert!(
            err.to_string().contains("spotify:12345"),
            "error message should contain id: {err}"
        );

        let bad_prefix_only = LocalPlaylist::new("local:", "Prefix Only");
        let err2 = store.save(&bad_prefix_only).unwrap_err();
        assert!(err2.to_string().contains("local:"), "error message should contain id: {err2}");
    }
}
