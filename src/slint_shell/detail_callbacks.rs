//! Detail view callbacks: track selection, play and shuffle of the open container.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::domain::{
    Detail, DetailCardKind, DetailKind, SortColumn, Track, is_local_playlist_id, next_sort,
};
use crate::slint_shell::callbacks::with_window_state;
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::import_callbacks::{current_store, reload_local_library};
use crate::slint_shell::import_state::local_detail;
use crate::slint_shell::local_edit;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::sync_window;
use crate::slint_shell::window::{open_detail_async, trigger_playback};
use crate::slint_shell::{DetailUi, LocalEditUi, MoggytWindow};

/// Wires detail view track selection and playback action callbacks.
pub(crate) fn wire_detail_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_track_selected(move |idx| {
        with_window_state(&w, &s, |win, st| handle_detail_track_selected(win, st, &tx, idx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_play(move || {
        with_window_state(&w, &s, |win, st| handle_detail_play(win, st, &tx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_shuffle(move || {
        with_window_state(&w, &s, |win, st| handle_detail_shuffle(win, st, &tx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_card_selected(move |idx| {
        with_window_state(&w, &s, |win, st| handle_detail_card_selected(win, st, &tx, idx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_show_all_songs(move || {
        with_window_state(&w, &s, |win, st| handle_detail_show_all_songs(win, st, &tx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<DetailUi>().on_compact_toggled(move || {
        with_window_state(&w, &s, |win, st| {
            st.toggle_detail_compact_tracks();
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<DetailUi>().on_sort_column_clicked(move |index| {
        with_window_state(&w, &s, |win, st| handle_detail_sort_clicked(win, st, index));
    });

    let local_ui = window.global::<LocalEditUi>();

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    local_ui.on_rename_submitted(move |title| {
        with_window_state(&w, &s, |win, st| handle_local_rename(win, st, title.as_str()));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    local_ui.on_delete_requested(move || {
        with_window_state(&w, &s, handle_local_delete);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    local_ui.on_remove_track_requested(move |idx| {
        with_window_state(&w, &s, |win, st| handle_local_remove_track(win, st, idx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    local_ui.on_move_track_requested(move |from, to| {
        with_window_state(&w, &s, |win, st| handle_local_move_track(win, st, from, to));
    });
}

/// Returns the playlist identifier from active detail request only if it is a local playlist.
pub(crate) fn current_local_detail_id(state: &SlintAppState) -> Option<String> {
    state
        .detail_request
        .as_ref()
        .filter(|(id, _)| is_local_playlist_id(id))
        .map(|(id, _)| id.clone())
}

fn handle_detail_sort_clicked(win: &MoggytWindow, st: &mut SlintAppState, index: i32) {
    let Some(column) = SortColumn::from_index(index) else {
        return;
    };
    let Some((request, _)) = st.detail_request.clone() else {
        return;
    };
    let current = st.detail_sort.as_ref().filter(|(id, _)| id == &request).map(|(_, sort)| *sort);
    st.detail_sort = next_sort(current, column).map(|sort| (request, sort));
    sync_window(win, st);
}

fn handle_local_rename(win: &MoggytWindow, st: &mut SlintAppState, title: &str) {
    let (Some(id), Some(store)) = (current_local_detail_id(st), current_store()) else {
        return;
    };
    match local_edit::rename_local_playlist(store.as_ref(), &id, title) {
        Ok(playlist) => {
            st.detail = Some(local_detail(&playlist));
            st.detail_error = None;
            reload_local_library(win, store.as_ref());
            sync_window(win, st);
        }
        Err(err) => {
            st.detail_error = Some(err.to_string());
            sync_window(win, st);
        }
    }
}

fn handle_local_delete(win: &MoggytWindow, st: &mut SlintAppState) {
    let (Some(id), Some(store)) = (current_local_detail_id(st), current_store()) else {
        return;
    };
    match local_edit::delete_local_playlist(store.as_ref(), &id) {
        Ok(()) => {
            st.page = st.detail_return;
            st.detail = None;
            st.detail_error = None;
            reload_local_library(win, store.as_ref());
            sync_window(win, st);
        }
        Err(err) => {
            st.detail_error = Some(err.to_string());
            sync_window(win, st);
        }
    }
}

fn handle_local_remove_track(win: &MoggytWindow, st: &mut SlintAppState, idx: i32) {
    if idx < 0 {
        return;
    }
    let (Some(id), Some(store)) = (current_local_detail_id(st), current_store()) else {
        return;
    };
    match local_edit::remove_local_track(store.as_ref(), &id, idx as usize) {
        Ok(playlist) => {
            st.detail = Some(local_detail(&playlist));
            st.detail_error = None;
            reload_local_library(win, store.as_ref());
            sync_window(win, st);
        }
        Err(err) => {
            st.detail_error = Some(err.to_string());
            sync_window(win, st);
        }
    }
}

fn handle_local_move_track(win: &MoggytWindow, st: &mut SlintAppState, from: i32, to: i32) {
    if from < 0 || to < 0 {
        return;
    }
    let (Some(id), Some(store)) = (current_local_detail_id(st), current_store()) else {
        return;
    };
    match local_edit::reorder_local_track(store.as_ref(), &id, from as usize, to as usize) {
        Ok(playlist) => {
            st.detail = Some(local_detail(&playlist));
            st.detail_error = None;
            reload_local_library(win, store.as_ref());
            sync_window(win, st);
        }
        Err(err) => {
            st.detail_error = Some(err.to_string());
            sync_window(win, st);
        }
    }
}

/// Replaces the playback queue with the open detail container's tracks and sets the cursor.
///
/// Requests cover art for every track and resets any active crossfade checkpoint.
/// Returns the replaced tracks, or `None` when no detail is open or its playback list is empty.
fn replace_detail_queue(
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    cursor: usize,
) -> Option<Vec<Track>> {
    let tracks = st.detail.as_ref()?.playback_tracks().to_vec();
    if tracks.is_empty() {
        return None;
    }
    for track in &tracks {
        request_queue_track_cover(
            track.artwork.as_deref(),
            st.play_generation,
            &st.search_covers,
            tx,
        );
    }
    st.replace_context(tracks.clone(), cursor);
    Some(tracks)
}

/// Resolves the playback-list index for a displayed track row.
///
/// Artist pages display the popular-songs shelf while the queue carries the complete
/// songs list, so the row is matched by track id before falling back to its position.
fn playback_index_for_row(detail: &Detail, row: usize) -> Option<usize> {
    let track = detail.tracks.get(row)?;
    let playback = detail.playback_tracks();
    if let Some(index) = playback.iter().position(|candidate| candidate.id == track.id) {
        return Some(index);
    }
    (row < playback.len()).then_some(row)
}

/// Replaces the playback queue with the open detail container's tracks and places the cursor on
/// the track matching row `idx`.
///
/// Returns the mapped track index within the replaced queue, or `None` when no detail is open
/// or the index does not address a track.
fn replace_detail_at_row(
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    idx: usize,
) -> Option<usize> {
    let target = playback_index_for_row(st.detail.as_ref()?, idx)?;
    replace_detail_queue(st, tx, target)?;
    Some(target)
}

fn handle_detail_track_selected(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    idx: i32,
) {
    if idx >= 0 && replace_detail_at_row(st, tx, idx as usize).is_some() {
        st.persist_queue();
        trigger_playback(win, st);
    }
    sync_window(win, st);
}

fn handle_detail_play(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.detail.as_ref().is_some_and(|d| !d.tracks.is_empty())
        && replace_detail_queue(st, tx, 0).is_some()
    {
        st.persist_queue();
        trigger_playback(win, st);
    }
    sync_window(win, st);
}

fn handle_detail_shuffle(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.detail.as_ref().is_some_and(|d| !d.tracks.is_empty()) {
        shuffle_detail_into_queue(st, tx);
        st.persist_queue();
        trigger_playback(win, st);
    }
    sync_window(win, st);
}

fn handle_detail_card_selected(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    idx: i32,
) {
    let Some(card) = st
        .detail
        .as_ref()
        .and_then(|detail| crate::slint_shell::detail::detail_card_at(detail, idx))
    else {
        return;
    };
    let id = card.id.clone();
    let kind = match card.kind {
        DetailCardKind::Album => DetailKind::Album,
        DetailCardKind::Artist => DetailKind::Artist,
    };
    open_detail_async(win, st, &id, kind, tx);
}

fn handle_detail_show_all_songs(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(id) = st
        .detail
        .as_ref()
        .and_then(|detail| detail.artist.as_ref())
        .and_then(|page| page.songs_more_id.clone())
    else {
        return;
    };
    open_detail_async(win, st, &id, DetailKind::Playlist, tx);
}

/// Replaces the playback queue with the open detail container's tracks and shuffles them with a
/// fully random permutation, recording the unshuffled baseline and setting cursor to 0.
fn shuffle_detail_into_queue(
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(tracks) = replace_detail_queue(st, tx, 0) else {
        return;
    };
    st.unshuffled_tracks = Some(tracks);
    st.is_shuffle = true;
    let order = crate::slint_shell::playback::shuffle_permutation(st.queue.len(), None);
    st.queue.shuffle_with(&order);
    st.play_track_at_index_in_memory(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Detail, DetailKind, Track};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_detail_hero_enqueue_tracks() {
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1").with_duration(180);
        let track2 = Track::new("t2", "Track 2", "Artist 2").with_duration(200);
        let detail =
            Detail::new(DetailKind::Album, "Album Title", "Album Subtitle", vec![track1, track2]);
        state.detail = Some(detail);

        let tracks = state.detail.as_ref().unwrap().tracks.clone();
        for t in tracks {
            state.queue.push(t);
        }
        state.play_track_at_index_in_memory(0);
        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    }

    #[test]
    fn test_detail_track_row_queues_container_and_starts_at_row() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = Some(Detail::new(
            DetailKind::Artist,
            "Artist",
            "",
            vec![
                Track::new("t1", "Track 1", "Artist"),
                Track::new("t2", "Track 2", "Artist"),
                Track::new("t3", "Track 3", "Artist"),
            ],
        ));

        assert_eq!(replace_detail_at_row(&mut state, &tx, 1), Some(1));
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2", "t3"]);
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
    }

    #[test]
    fn test_detail_track_row_replaces_existing_queue() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.queue.push(Track::new("old", "Old", "Other"));
        state.detail = Some(Detail::new(
            DetailKind::Album,
            "Album",
            "",
            vec![Track::new("a1", "A1", "Artist"), Track::new("a2", "A2", "Artist")],
        ));

        assert_eq!(replace_detail_at_row(&mut state, &tx, 1), Some(1));
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["a1", "a2"]);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("a2"));
    }

    fn artist_detail_with_full_songs_list() -> Detail {
        let shelf = vec![Track::new("t1", "One", "Artist"), Track::new("t2", "Two", "Artist")];
        let complete = vec![
            Track::new("t0", "Zero", "Artist"),
            Track::new("t1", "One", "Artist"),
            Track::new("t2", "Two", "Artist"),
            Track::new("t3", "Three", "Artist"),
        ];
        Detail::new(DetailKind::Artist, "Artist", "", shelf).with_queue_tracks(complete)
    }

    #[test]
    fn test_detail_play_queues_the_full_artist_songs_list() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = Some(artist_detail_with_full_songs_list());

        replace_detail_queue(&mut state, &tx, 0);

        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t0", "t1", "t2", "t3"]);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t0"));
    }

    #[test]
    fn test_detail_track_row_matches_the_full_list_track_id() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = Some(artist_detail_with_full_songs_list());

        assert_eq!(replace_detail_at_row(&mut state, &tx, 1), Some(2));
        assert_eq!(state.queue.len(), 4);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));
    }

    #[test]
    fn test_detail_track_row_falls_back_to_its_position_when_unmatched() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        let shelf = vec![Track::new("shelf-only", "Shelf", "Artist")];
        let complete = vec![Track::new("t1", "One", "Artist"), Track::new("t2", "Two", "Artist")];
        state.detail =
            Some(Detail::new(DetailKind::Artist, "Artist", "", shelf).with_queue_tracks(complete));

        assert_eq!(replace_detail_at_row(&mut state, &tx, 0), Some(0));
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    }

    #[test]
    fn test_detail_shuffle_uses_the_full_artist_songs_list() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = Some(artist_detail_with_full_songs_list());

        shuffle_detail_into_queue(&mut state, &tx);

        assert_eq!(state.queue.len(), 4);
        assert_eq!(state.unshuffled_tracks.as_ref().map(Vec::len), Some(4));
        assert_eq!(state.queue.cursor(), 0);
    }

    #[test]
    fn test_detail_track_row_out_of_range_does_not_enqueue() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = Some(Detail::new(
            DetailKind::Album,
            "Album",
            "",
            vec![Track::new("a1", "A1", "Artist")],
        ));

        assert_eq!(replace_detail_at_row(&mut state, &tx, 1), None);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_detail_track_row_without_detail_is_noop() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();

        assert_eq!(replace_detail_at_row(&mut state, &tx, 0), None);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_handle_detail_shuffle_logic() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1").with_duration(180);
        let track2 = Track::new("t2", "Track 2", "Artist 2").with_duration(200);
        let track3 = Track::new("t3", "Track 3", "Artist 3").with_duration(220);
        let detail = Detail::new(
            DetailKind::Album,
            "Album Title",
            "Album Subtitle",
            vec![track1, track2, track3],
        );
        state.detail = Some(detail);

        if let Ok(win) = MoggytWindow::new() {
            handle_detail_shuffle(&win, &mut state, &tx);
            assert!(state.is_shuffle);
            assert_eq!(state.queue.len(), 3);
            assert!(state.unshuffled_tracks.is_some());
            assert_eq!(state.unshuffled_tracks.as_ref().unwrap().len(), 3);
            assert_eq!(state.queue.cursor(), 0);
            assert!(state.queue.current().is_some());
        } else {
            shuffle_detail_into_queue(&mut state, &tx);
            assert_eq!(state.queue.len(), 3);
            assert!(state.unshuffled_tracks.is_some());
            assert_eq!(state.unshuffled_tracks.as_ref().unwrap().len(), 3);
            assert_eq!(state.queue.cursor(), 0);
            assert!(state.queue.current().is_some());
        }
    }

    #[test]
    fn test_handle_detail_shuffle_empty_detail_noop() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.detail = None;

        if let Ok(win) = MoggytWindow::new() {
            handle_detail_shuffle(&win, &mut state, &tx);
            assert!(!state.is_shuffle);
            assert!(state.queue.is_empty());
            assert!(state.unshuffled_tracks.is_none());
        }
    }

    #[test]
    fn test_current_local_detail_id() {
        let mut state = test_wired();
        assert_eq!(current_local_detail_id(&state), None);

        state.detail_request = Some(("spotify:abc".to_string(), DetailKind::Playlist));
        assert_eq!(current_local_detail_id(&state), None);

        state.detail_request = Some(("local:my_playlist".to_string(), DetailKind::Playlist));
        assert_eq!(current_local_detail_id(&state), Some("local:my_playlist".to_string()));
    }

    #[test]
    fn test_detail_shuffle_replaces_queue_and_sets_unshuffled_snapshot() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1").with_duration(180);
        let track2 = Track::new("t2", "Track 2", "Artist 2").with_duration(200);
        state.queue.push(track1.clone());
        state.queue.push(track2.clone());
        state.unshuffled_tracks = Some(vec![track2, track1]);
        state.is_shuffle = true;

        state.detail = Some(Detail::new(
            DetailKind::Album,
            "Album Title",
            "Album Subtitle",
            vec![
                Track::new("d1", "Detail 1", "Artist D").with_duration(210),
                Track::new("d2", "Detail 2", "Artist D").with_duration(220),
            ],
        ));
        shuffle_detail_into_queue(&mut state, &tx);

        let snapshot: Vec<&str> = state
            .unshuffled_tracks
            .as_ref()
            .expect("snapshot present")
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(snapshot, vec!["d1", "d2"]);

        state.toggle_shuffle();
        assert!(!state.is_shuffle);
        let restored: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(restored, vec!["d1", "d2"]);
    }

    #[test]
    fn test_detail_play_replaces_restored_queue_with_album_tracks() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.queue.push(Track::new("restored-1", "Restored 1", "Artist"));
        state.queue.push(Track::new("restored-2", "Restored 2", "Artist"));
        state.queue.push(Track::new("autoplay-1", "Autoplay 1", "Artist"));
        state.last_crossfaded_track = Some("restored-1".to_string());

        let track1 = Track::new("album-1", "Album Track 1", "Artist").with_duration(180);
        let track2 = Track::new("album-2", "Album Track 2", "Artist").with_duration(200);
        state.detail =
            Some(Detail::new(DetailKind::Album, "Album Title", "Artist", vec![track1, track2]));

        if let Ok(win) = MoggytWindow::new() {
            handle_detail_play(&win, &mut state, &tx);
        } else {
            replace_detail_queue(&mut state, &tx, 0);
        }

        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["album-1", "album-2"]);
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("album-1"));
        assert_eq!(state.last_crossfaded_track, None);
    }

    #[test]
    fn test_detail_play_with_shuffle_enabled_sets_baseline_and_keeps_tracks() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.is_shuffle = true;
        state.unshuffled_tracks = Some(vec![Track::new("old", "Old", "Artist")]);

        let track1 = Track::new("album-1", "Track 1", "Artist");
        let track2 = Track::new("album-2", "Track 2", "Artist");
        state.detail =
            Some(Detail::new(DetailKind::Album, "Album", "Artist", vec![track1, track2]));

        if let Ok(win) = MoggytWindow::new() {
            handle_detail_play(&win, &mut state, &tx);
        } else {
            replace_detail_queue(&mut state, &tx, 0);
        }

        assert_eq!(state.queue.len(), 2);
        let mut current_ids: Vec<&str> =
            state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        current_ids.sort();
        assert_eq!(current_ids, ["album-1", "album-2"]);

        let baseline_ids: Vec<&str> = state
            .unshuffled_tracks
            .as_ref()
            .expect("unshuffled baseline present")
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(baseline_ids, ["album-1", "album-2"]);
    }

    #[test]
    fn test_detail_play_and_shuffle_persist_queue() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        let track1 = Track::new("album-1", "Track 1", "Artist");
        let track2 = Track::new("album-2", "Track 2", "Artist");
        state.detail =
            Some(Detail::new(DetailKind::Album, "Album", "Artist", vec![track1, track2]));

        if let Ok(win) = MoggytWindow::new() {
            handle_detail_play(&win, &mut state, &tx);
            assert_eq!(state.queue_store.load().len(), 2);
            assert_eq!(state.queue_store.load().cursor(), 0);

            handle_detail_shuffle(&win, &mut state, &tx);
            assert_eq!(state.queue_store.load().len(), 2);
            assert_eq!(state.queue_store.load().cursor(), 0);
        }
    }
}
