//! User interaction callbacks and event dispatch wiring for MoggytWindow.
//!
//! Hexagonal invariant: pure in-memory UI event bindings delegating to asynchronous worker
//! dispatchers and state mutations without direct I/O.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::domain::{DetailKind, PlaybackPhase, SearchFilter, SearchResult};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::explore::{
    explore_item_at, load_category_async, load_explore_async, refresh_explore_covers_if_loaded,
};
use crate::slint_shell::format::{
    audio_quality_from_index, audio_quality_to_index, format_playback_speed,
};
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::playback::{
    cycle_repeat_mode, format_position, progress_fraction, repeat_mode_to_int, should_seek,
    toggle_like,
};
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::search::iter_shelf_items;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::{
    persist_recent_searches, sync_recent_searches, sync_shelves_and_queue, sync_window,
};
use crate::slint_shell::window::{
    clear_search_and_sync, open_detail_async, refresh_library_async, submit_search_async,
    submit_search_with_filter_async, trigger_playback,
};
use crate::slint_shell::{MoggytWindow, PlaybackUi, PlayerUi};

/// Maps a clicked search result item to playback or container detail view.
pub(crate) fn handle_filtered_item_click(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    item: SearchResult,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    match item {
        SearchResult::Track(track) => {
            request_queue_track_cover(
                track.artwork.as_deref(),
                state.play_generation,
                &state.search_covers,
                worker_tx,
            );
            state.queue.push(track);
            let last_idx = state.queue.len().saturating_sub(1);
            state.play_track_at_index_in_memory(last_idx);
            state.page = SlintPage::NowPlaying;
            trigger_playback(window, state);
            sync_window(window, state);
        }
        SearchResult::Album(album) => {
            open_detail_async(window, state, &album.id, DetailKind::Album, worker_tx);
        }
        SearchResult::Artist(artist) => {
            open_detail_async(window, state, &artist.id, DetailKind::Artist, worker_tx);
        }
        SearchResult::Playlist(playlist) => {
            open_detail_async(window, state, &playlist.id, DetailKind::Playlist, worker_tx);
        }
    }
}

/// Routes an explore or category item click to a category shelves page or standard detail/playback.
pub(crate) fn handle_explore_item_click(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    item: SearchResult,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if let SearchResult::Playlist(ref p) = item {
        if p.id.starts_with("FEmusic_") {
            load_category_async(window, state, &p.id, p.params.as_deref(), &p.title, worker_tx);
            return;
        }
    }
    handle_filtered_item_click(window, state, item, worker_tx);
}

/// Non-blocking handler for playback toggle (pause, resume, or start head track).
pub(crate) fn handle_toggle_play(window: &MoggytWindow, state: &mut SlintAppState) {
    let audio_arc = std::sync::Arc::clone(&state.audio);
    let Ok(mut audio) = audio_arc.try_lock() else {
        return;
    };
    if audio.is_playing() {
        audio.pause();
        drop(audio);
        state.pause_play_ledger();
        state.set_playback_phase(PlaybackPhase::Paused);
        state.sync_presence_playing(false);
        window.global::<PlayerUi>().set_is_playing(false);
        return;
    }
    if audio.last_requested().is_some() {
        audio.resume_play();
        drop(audio);
        state.resume_play_ledger();
        state.set_playback_phase(PlaybackPhase::Resolving);
        state.sync_presence_playing(true);
        window.global::<PlayerUi>().set_is_playing(true);
        return;
    }
    drop(audio);
    trigger_playback(window, state);
}

/// Wires user event callbacks between MoggytWindow and the application state without blocking.
pub(crate) fn wire_window_callbacks(
    window: &MoggytWindow,
    state_rc: SharedState,
    worker_tx: std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    wire_navigation_callbacks(window, &state_rc, &worker_tx);
    wire_search_callbacks(window, &state_rc, &worker_tx);
    wire_library_callbacks(window, &state_rc, &worker_tx);
    super::detail_callbacks::wire_detail_callbacks(window, &state_rc, &worker_tx);
    wire_playback_callbacks(window, &state_rc, &worker_tx);
    super::settings_callbacks::wire_settings_callbacks(window, &state_rc, &worker_tx);
    super::shortcuts::wire_shortcut_callbacks(window);
    super::discord::wire_discord_callbacks(window, &state_rc);
    super::lastfm_connect::wire_lastfm_callbacks(window, &state_rc);
    super::spotify_connect::wire_spotify_callbacks(window, &state_rc);
    super::stats::wire_stats_callbacks(window, &state_rc);
    super::version_fix::wire(window, state_rc.clone());
}

fn handle_page_selected(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    page: SlintPage,
) {
    match page {
        SlintPage::Library => refresh_library_async(win, st, tx),
        SlintPage::Explore if !st.explore_loaded && !st.explore_loading => {
            load_explore_async(win, st, tx)
        }
        SlintPage::Explore => {
            refresh_explore_covers_if_loaded(st, tx);
        }
        SlintPage::Stats => super::stats::refresh_active_stats(win, st),
        _ => {}
    }
}

/// Wires page navigation, back navigation, and queue panel drawer visibility callbacks.
fn wire_navigation_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_page_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let page = SlintPage::from_index(idx);
        st.page = page;
        handle_page_selected(&win, &mut st, &tx, page);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_back_pressed(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.go_back();
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_toggle_queue_panel(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.toggle_queue_panel();
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_close_queue_panel(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.close_queue_panel();
        sync_window(&win, &mut st);
    });
}

/// Wires search query, filter selection, and result interaction callbacks.
fn wire_search_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_query_submitted(move |q| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        submit_search_async(&win, &mut st, q.as_str(), &tx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_recent_search_selected(move |q| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        submit_search_async(&win, &mut st, q.as_str(), &tx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_recent_search_removed(move |q| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.recent_searches.remove(q.as_str());
        persist_recent_searches(&st);
        sync_recent_searches(&win, &st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_recent_searches_cleared(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.recent_searches.clear();
        persist_recent_searches(&st);
        sync_recent_searches(&win, &st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_query_cleared(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        clear_search_and_sync(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let item_opt =
            iter_shelf_items(&st.home_shelves, st.search_filter).nth(idx as usize).cloned();
        if let Some(item) = item_opt {
            handle_filtered_item_click(&win, &mut st, item, &tx);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_explore_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let item_opt = explore_item_at(&st.explore_shelves, idx as usize).cloned();
        if let Some(item) = item_opt {
            handle_explore_item_click(&win, &mut st, item, &tx);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_category_item_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let item_opt = explore_item_at(&st.category_shelves, idx as usize).cloned();
        if let Some(item) = item_opt {
            handle_explore_item_click(&win, &mut st, item, &tx);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_filter_selected(move |filter_idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let filter = filter_from_index(filter_idx);
        st.lyrics_search = filter_idx == 5;
        st.search_filter = filter;
        win.set_search_filter(filter_idx);
        if st.search_active && !st.search_query.trim().is_empty() {
            let query = st.search_query.clone();
            submit_search_with_filter_async(&win, &mut st, &query, filter, &tx);
            return;
        }
        sync_shelves_and_queue(&win, &st);
    });
}

fn filter_from_index(filter_idx: i32) -> SearchFilter {
    match filter_idx {
        1 => SearchFilter::Tracks,
        2 => SearchFilter::Albums,
        3 => SearchFilter::Artists,
        4 => SearchFilter::Playlists,
        5 => SearchFilter::Tracks,
        _ => SearchFilter::All,
    }
}

/// Wires library container and queue selection callbacks.
fn wire_library_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_queue_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.play_track_at_index_in_memory(idx as usize);
        trigger_playback(&win, &mut st);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_playlist_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(p) = st.library_playlists.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &p.id, DetailKind::Playlist, &tx);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_album_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(a) = st.library_albums.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &a.id, DetailKind::Album, &tx);
        }
    });
}

/// Runs `f` with the upgraded window and the borrowed app state, ignoring stale handles.
pub(crate) fn with_window_state<F>(w: &slint::Weak<MoggytWindow>, s: &SharedState, f: F)
where
    F: FnOnce(&MoggytWindow, &mut SlintAppState),
{
    let Some(win) = w.upgrade() else { return };
    let mut st = s.borrow_mut();
    f(&win, &mut st);
}

/// Wires playback transport, track stepping, seek, volume, and preference callbacks.
fn wire_playback_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_toggle_play(move || {
        with_window_state(&w, &s, handle_toggle_play);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_prev_track(move || {
        with_window_state(&w, &s, handle_prev_track);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_next_track(move || {
        with_window_state(&w, &s, handle_next_track);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_seek_by(move |delta| {
        with_window_state(&w, &s, |win, st| {
            st.seek_by(delta as i64);
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.global::<PlayerUi>().on_seek_to(move |secs| {
        with_window_state(&w, &s, |win, st| handle_seek_to(win, st, &tx, secs));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_set_volume(move |vol| {
        with_window_state(&w, &s, |win, st| {
            st.set_volume(vol.clamp(0, 100) as u8);
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_toggle_like(move || {
        with_window_state(&w, &s, handle_toggle_like);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_toggle_shuffle(move || {
        with_window_state(&w, &s, |win, st| {
            st.toggle_shuffle();
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlayerUi>().on_cycle_repeat(move || {
        with_window_state(&w, &s, handle_cycle_repeat);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_cycle_playback_speed(move || {
        with_window_state(&w, &s, handle_cycle_playback_speed);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_audio_quality_selected(move |idx| {
        with_window_state(&w, &s, |win, st| handle_audio_quality_selected(win, st, idx));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_songs_only_toggled(move || {
        with_window_state(&w, &s, |win, st| {
            st.toggle_songs_only();
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlaybackUi>().on_autoplay_toggled(move || {
        with_window_state(&w, &s, |win, st| {
            st.toggle_autoplay();
            sync_window(win, st);
        });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<PlaybackUi>().on_queue_cleared(move || {
        with_window_state(&w, &s, |win, st| {
            st.clear_queue();
            sync_window(win, st);
        });
    });
}

fn handle_prev_track(win: &MoggytWindow, st: &mut SlintAppState) {
    st.queue.retreat();
    trigger_playback(win, st);
    sync_window(win, st);
}

fn handle_next_track(win: &MoggytWindow, st: &mut SlintAppState) {
    st.queue.advance();
    trigger_playback(win, st);
    sync_window(win, st);
}

fn handle_seek_to(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
    secs: i32,
) {
    let target = secs.max(0) as u64;
    if let Ok(current) = st.audio.try_lock().map(|a| a.position_secs()) {
        if !should_seek(target, current) {
            return;
        }
    }
    let _ = tx.send(PlaybackWorkerCommand::Seek { position_secs: target });
    let player = win.global::<PlayerUi>();
    player.set_position_secs(target as i32);
    player.set_position_text(format_position(target as u32).into());
    let dur = st.queue.current().and_then(|t| t.duration_secs).unwrap_or(0);
    player.set_progress(progress_fraction(target as u32, dur));
}

fn handle_toggle_like(win: &MoggytWindow, st: &mut SlintAppState) {
    if let Some(track) = st.queue.current() {
        let id = track.id.clone();
        let is_liked = toggle_like(&mut st.liked_ids, &id);
        win.global::<PlayerUi>().set_is_liked(is_liked);
    }
}

fn handle_cycle_repeat(win: &MoggytWindow, st: &mut SlintAppState) {
    let next = cycle_repeat_mode(st.queue.repeat());
    st.queue.set_repeat(next);
    win.global::<PlayerUi>().set_repeat_mode(repeat_mode_to_int(next));
}

fn handle_cycle_playback_speed(win: &MoggytWindow, st: &mut SlintAppState) {
    let speed = st.cycle_playback_speed();
    win.set_playback_speed_text(format_playback_speed(speed).into());
}

fn handle_audio_quality_selected(win: &MoggytWindow, st: &mut SlintAppState, idx: i32) {
    let quality = audio_quality_from_index(idx);
    st.set_audio_quality(quality);
    win.set_active_audio_quality_index(audio_quality_to_index(quality));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::slint_shell::playback::is_track_liked;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_seek_feedback_guard_condition() {
        let state = test_wired();
        state.audio.lock().unwrap().seek(50);
        let current = state.audio.try_lock().map(|a| a.position_secs()).unwrap();
        assert_eq!(current, 50);

        assert!(!should_seek(50, current));
        assert!(!should_seek(51, current));
        assert!(!should_seek(49, current));

        assert!(should_seek(52, current));
        assert!(should_seek(48, current));
        assert!(should_seek(0, current));
    }

    #[test]
    fn test_like_toggle_and_state_persistence() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        assert!(!is_track_liked(&state.liked_ids, "t1"));

        toggle_like(&mut state.liked_ids, "t1");
        assert!(is_track_liked(&state.liked_ids, "t1"));

        toggle_like(&mut state.liked_ids, "t1");
        assert!(!is_track_liked(&state.liked_ids, "t1"));
    }

    #[test]
    fn test_repeat_mode_cycling_in_state() {
        let mut state = test_wired();
        assert_eq!(state.queue.repeat(), crate::domain::RepeatMode::Off);

        let m1 = cycle_repeat_mode(state.queue.repeat());
        state.queue.set_repeat(m1);
        assert_eq!(state.queue.repeat(), crate::domain::RepeatMode::All);

        let m2 = cycle_repeat_mode(state.queue.repeat());
        state.queue.set_repeat(m2);
        assert_eq!(state.queue.repeat(), crate::domain::RepeatMode::One);

        let m3 = cycle_repeat_mode(state.queue.repeat());
        state.queue.set_repeat(m3);
        assert_eq!(state.queue.repeat(), crate::domain::RepeatMode::Off);
    }

    #[test]
    fn test_toggle_shuffle_callback_action() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
        assert!(!state.is_shuffle);

        state.toggle_shuffle();
        assert!(state.is_shuffle);
        assert!(state.unshuffled_tracks.is_some());

        state.toggle_shuffle();
        assert!(!state.is_shuffle);
        assert!(state.unshuffled_tracks.is_none());
    }

    #[test]
    fn test_filter_from_index_mapping() {
        assert_eq!(filter_from_index(0), SearchFilter::All);
        assert_eq!(filter_from_index(1), SearchFilter::Tracks);
        assert_eq!(filter_from_index(2), SearchFilter::Albums);
        assert_eq!(filter_from_index(3), SearchFilter::Artists);
        assert_eq!(filter_from_index(4), SearchFilter::Playlists);
        assert_eq!(filter_from_index(5), SearchFilter::Tracks);
        assert_eq!(filter_from_index(99), SearchFilter::All);
    }
}
