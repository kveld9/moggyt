//! Detail view callbacks: track selection, play and shuffle of the open container.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::domain::{DetailCardKind, DetailKind, is_local_playlist_id};
use crate::slint_shell::callbacks::with_window_state;
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::import_callbacks::{current_store, reload_local_library};
use crate::slint_shell::import_state::local_detail;
use crate::slint_shell::local_edit;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::sync_window;
use crate::slint_shell::window::{open_detail_async, trigger_playback};
use crate::slint_shell::{LocalEditUi, MoggytWindow};

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

fn enqueue_detail_tracks(
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) -> usize {
    let Some(detail) = st.detail.clone() else {
        return st.queue.len();
    };
    let start_idx = st.queue.len();
    for track in &detail.tracks {
        request_queue_track_cover(
            track.artwork.as_deref(),
            st.play_generation,
            &st.search_covers,
            tx,
        );
    }
    for track in detail.tracks {
        st.queue.push(track);
    }
    start_idx
}

fn handle_detail_track_selected(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    idx: i32,
) {
    if let Some(detail) = st.detail.clone() {
        if let Some(track) = detail.tracks.get(idx as usize).cloned() {
            request_queue_track_cover(
                track.artwork.as_deref(),
                st.play_generation,
                &st.search_covers,
                tx,
            );
            st.queue.push(track);
            let last = st.queue.len().saturating_sub(1);
            st.play_track_at_index_in_memory(last);
            trigger_playback(win, st);
        }
    }
    sync_window(win, st);
}

fn handle_detail_play(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.detail.as_ref().is_some_and(|d| !d.tracks.is_empty()) {
        let start = enqueue_detail_tracks(st, tx);
        st.play_track_at_index_in_memory(start);
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

/// Enqueues the open detail and shuffles it, rebuilding the linear queue first so the
/// unshuffled baseline always holds every queued track in its real order.
fn shuffle_detail_into_queue(
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.is_shuffle {
        st.restore_unshuffled_queue();
    }
    st.is_shuffle = true;
    let start = enqueue_detail_tracks(st, tx);
    st.unshuffled_tracks = Some(st.queue.tracks().to_vec());
    let order = crate::slint_shell::playback::shuffle_permutation(st.queue.len(), Some(start));
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
            assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
        } else {
            shuffle_detail_into_queue(&mut state, &tx);
            assert_eq!(state.queue.len(), 3);
            assert!(state.unshuffled_tracks.is_some());
            assert_eq!(state.unshuffled_tracks.as_ref().unwrap().len(), 3);
            assert_eq!(state.queue.cursor(), 0);
            assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
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
    fn test_detail_shuffle_keeps_original_order_when_already_shuffled() {
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
            vec![Track::new("d1", "Detail 1", "Artist D").with_duration(210)],
        ));
        shuffle_detail_into_queue(&mut state, &tx);

        let snapshot: Vec<&str> = state
            .unshuffled_tracks
            .as_ref()
            .expect("snapshot present")
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(snapshot, vec!["t2", "t1", "d1"]);

        state.toggle_shuffle();
        assert!(!state.is_shuffle);
        let restored: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(restored, vec!["t2", "t1", "d1"]);
    }
}
