//! Pure mutation operations for local imported playlists.
//!
//! Provides validation and mutation helpers for renaming, deleting,
//! removing tracks, and reordering tracks within an existing [`PlaylistStore`].
//!
//! Hexagonal invariant: this module performs zero direct disk I/O, network calls,
//! or async runtimes; all persistence is mediated through the [`PlaylistStore`] port.

use crate::domain::{LocalPlaylist, LocalTrack, is_local_playlist_id};
use crate::infra::PlaylistStore;

/// Renames a local playlist after trimming and validating the title.
///
/// Rejects non-local identifiers and blank/whitespace titles.
/// Fails if the playlist does not exist in the store.
pub fn rename_local_playlist(
    store: &dyn PlaylistStore,
    id: &str,
    title: &str,
) -> anyhow::Result<LocalPlaylist> {
    validate_local_id(id)?;
    let clean_title = validate_title(title)?;
    let mut playlist = load_playlist(store, id)?;
    playlist.title = clean_title.to_owned();
    store.save(&playlist)?;
    Ok(playlist)
}

/// Deletes a local playlist from the store.
///
/// Rejects non-local identifiers with an error naming the identifier.
/// Deleting an unknown but valid local identifier is tolerated and succeeds.
pub fn delete_local_playlist(store: &dyn PlaylistStore, id: &str) -> anyhow::Result<()> {
    validate_local_id(id)?;
    store.delete(id)
}

/// Removes a track by index from a local playlist.
///
/// Rejects non-local identifiers and out-of-range track indices without mutating the store.
/// Fails if the playlist does not exist in the store.
pub fn remove_local_track(
    store: &dyn PlaylistStore,
    id: &str,
    index: usize,
) -> anyhow::Result<LocalPlaylist> {
    validate_local_id(id)?;
    let mut playlist = load_playlist(store, id)?;
    validate_track_index(playlist.tracks.len(), index, "track")?;
    playlist.tracks.remove(index);
    store.save(&playlist)?;
    Ok(playlist)
}

/// Reorders a track within a local playlist from one index to another.
///
/// Rejects non-local identifiers and out-of-range `from` or `to` indices.
/// Reordering to the same index is a no-op that still saves and returns the playlist.
/// Fails if the playlist does not exist in the store.
pub fn reorder_local_track(
    store: &dyn PlaylistStore,
    id: &str,
    from: usize,
    to: usize,
) -> anyhow::Result<LocalPlaylist> {
    validate_local_id(id)?;
    let mut playlist = load_playlist(store, id)?;
    let len = playlist.tracks.len();
    validate_track_index(len, from, "from")?;
    validate_track_index(len, to, "to")?;
    move_track(&mut playlist.tracks, from, to);
    store.save(&playlist)?;
    Ok(playlist)
}

fn validate_local_id(id: &str) -> anyhow::Result<()> {
    if !is_local_playlist_id(id) {
        anyhow::bail!("invalid local playlist id '{id}'");
    }
    Ok(())
}

fn validate_title(title: &str) -> anyhow::Result<&str> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        anyhow::bail!("playlist title cannot be empty or whitespace");
    }
    Ok(trimmed)
}

fn validate_track_index(len: usize, index: usize, label: &str) -> anyhow::Result<()> {
    if index >= len {
        anyhow::bail!("{label} index {index} is out of bounds (playlist has {len} tracks)");
    }
    Ok(())
}

fn load_playlist(store: &dyn PlaylistStore, id: &str) -> anyhow::Result<LocalPlaylist> {
    store.get(id).ok_or_else(|| anyhow::anyhow!("local playlist not found: '{id}'"))
}

fn move_track(tracks: &mut Vec<LocalTrack>, from: usize, to: usize) {
    if from != to {
        let track = tracks.remove(from);
        tracks.insert(to, track);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MatchConfidence;
    use crate::infra::MemoryPlaylistStore;

    fn seed_three_track_playlist(store: &dyn PlaylistStore, id: &str) -> LocalPlaylist {
        let mut playlist = LocalPlaylist::new(id, "Original Title");
        playlist.tracks = vec![
            LocalTrack::new("v0", "Track 0", "Artist 0", MatchConfidence::High),
            LocalTrack::new("v1", "Track 1", "Artist 1", MatchConfidence::Medium),
            LocalTrack::new("v2", "Track 2", "Artist 2", MatchConfidence::Low),
        ];
        store.save(&playlist).expect("seed playlist save succeeds");
        playlist
    }

    #[test]
    fn test_rename_success_updates_title_in_return_and_store() {
        let store = MemoryPlaylistStore::new();
        seed_three_track_playlist(&store, "local:p1");

        let updated = rename_local_playlist(&store, "local:p1", "  Updated Title  ")
            .expect("rename succeeds");
        assert_eq!(updated.title, "Updated Title");

        let stored = store.get("local:p1").expect("stored playlist exists");
        assert_eq!(stored.title, "Updated Title");
        assert_eq!(stored.tracks.len(), 3);
    }

    #[test]
    fn test_rename_rejects_blank_titles_and_non_local_ids_without_mutating_store() {
        let store = MemoryPlaylistStore::new();
        let seeded = seed_three_track_playlist(&store, "local:p1");

        let blank_cases = ["", "   ", "\t\n\r"];
        for blank in blank_cases {
            let err = rename_local_playlist(&store, "local:p1", blank).unwrap_err();
            assert!(
                err.to_string().contains("empty or whitespace"),
                "expected blank title error, got: {err}"
            );
            assert_eq!(store.get("local:p1"), Some(seeded.clone()));
        }

        let non_local_cases = ["spotify:12345", "local:", "custom_id", ""];
        for bad_id in non_local_cases {
            let err = rename_local_playlist(&store, bad_id, "Valid Title").unwrap_err();
            assert!(
                err.to_string().contains(bad_id),
                "error message should name non-local id '{bad_id}': {err}"
            );
        }
        assert_eq!(store.get("local:p1"), Some(seeded));

        let missing_err =
            rename_local_playlist(&store, "local:missing", "Valid Title").unwrap_err();
        assert!(missing_err.to_string().contains("local:missing"));
    }

    #[test]
    fn test_delete_removes_playlist_and_tolerates_unknown_local_id() {
        let store = MemoryPlaylistStore::new();
        seed_three_track_playlist(&store, "local:p1");

        delete_local_playlist(&store, "local:unknown").expect("unknown local id tolerated");
        assert!(store.get("local:p1").is_some());

        delete_local_playlist(&store, "local:p1").expect("delete existing succeeds");
        assert_eq!(store.get("local:p1"), None);

        let err = delete_local_playlist(&store, "spotify:non_local").unwrap_err();
        assert!(
            err.to_string().contains("spotify:non_local"),
            "error should name non-local id: {err}"
        );
    }

    #[test]
    fn test_remove_track_success_preserves_order() {
        let store = MemoryPlaylistStore::new();
        seed_three_track_playlist(&store, "local:p1");

        let updated = remove_local_track(&store, "local:p1", 1).expect("remove index 1 succeeds");
        let remaining_ids: Vec<&str> = updated.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(remaining_ids, vec!["v0", "v2"]);

        let stored = store.get("local:p1").expect("stored playlist exists");
        let stored_ids: Vec<&str> = stored.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(stored_ids, vec!["v0", "v2"]);
    }

    #[test]
    fn test_remove_track_rejects_out_of_range_and_non_local_ids_without_mutation() {
        let store = MemoryPlaylistStore::new();
        let seeded = seed_three_track_playlist(&store, "local:p1");

        let err_oob = remove_local_track(&store, "local:p1", 3).unwrap_err();
        assert!(err_oob.to_string().contains("out of bounds"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_oob_large = remove_local_track(&store, "local:p1", 99).unwrap_err();
        assert!(err_oob_large.to_string().contains("out of bounds"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_non_local = remove_local_track(&store, "spotify:12345", 0).unwrap_err();
        assert!(err_non_local.to_string().contains("spotify:12345"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_missing = remove_local_track(&store, "local:missing", 0).unwrap_err();
        assert!(err_missing.to_string().contains("local:missing"));
        assert_eq!(store.get("local:p1"), Some(seeded));
    }

    #[test]
    fn test_reorder_track_success_moves_from_and_to_correctly() {
        let store = MemoryPlaylistStore::new();
        seed_three_track_playlist(&store, "local:p1");

        // Move track 0 to index 2: [v0, v1, v2] -> [v1, v2, v0]
        let moved_forward =
            reorder_local_track(&store, "local:p1", 0, 2).expect("reorder 0 to 2 succeeds");
        let ids: Vec<&str> = moved_forward.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, vec!["v1", "v2", "v0"]);
        let stored = store.get("local:p1").expect("stored playlist exists");
        let stored_ids: Vec<&str> = stored.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(stored_ids, vec!["v1", "v2", "v0"]);

        // Move track 2 to index 0: [v1, v2, v0] -> [v0, v1, v2]
        let moved_backward =
            reorder_local_track(&store, "local:p1", 2, 0).expect("reorder 2 to 0 succeeds");
        let ids_back: Vec<&str> =
            moved_backward.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids_back, vec!["v0", "v1", "v2"]);

        // No-op reorder to same index: [v0, v1, v2] -> [v0, v1, v2]
        let same_index =
            reorder_local_track(&store, "local:p1", 1, 1).expect("reorder 1 to 1 succeeds");
        let ids_same: Vec<&str> = same_index.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids_same, vec!["v0", "v1", "v2"]);
    }

    #[test]
    fn test_reorder_track_rejects_out_of_range_and_non_local_ids_without_mutation() {
        let store = MemoryPlaylistStore::new();
        let seeded = seed_three_track_playlist(&store, "local:p1");

        let err_from_oob = reorder_local_track(&store, "local:p1", 3, 0).unwrap_err();
        assert!(err_from_oob.to_string().contains("from index 3 is out of bounds"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_to_oob = reorder_local_track(&store, "local:p1", 0, 3).unwrap_err();
        assert!(err_to_oob.to_string().contains("to index 3 is out of bounds"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_non_local = reorder_local_track(&store, "spotify:non_local", 0, 1).unwrap_err();
        assert!(err_non_local.to_string().contains("spotify:non_local"));
        assert_eq!(store.get("local:p1"), Some(seeded.clone()));

        let err_missing = reorder_local_track(&store, "local:missing", 0, 1).unwrap_err();
        assert!(err_missing.to_string().contains("local:missing"));
        assert_eq!(store.get("local:p1"), Some(seeded));
    }
}
