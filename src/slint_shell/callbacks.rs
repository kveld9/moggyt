//! User interaction callbacks and event dispatch wiring for MoggytWindow.
//!
//! Hexagonal invariant: pure in-memory UI event bindings delegating to asynchronous worker
//! dispatchers and state mutations without direct I/O.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::domain::{DetailKind, SearchFilter, SearchResult};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::explore::{
    explore_columns_for_width, explore_item_at, load_explore_async,
    refresh_explore_covers_if_loaded,
};
use crate::slint_shell::format::format_playback_speed;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::playback::{
    cycle_repeat_mode, format_position, progress_fraction, repeat_mode_to_int, should_seek,
    toggle_like,
};
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::player_bar::mode_from_index;
use crate::slint_shell::search::iter_shelf_items;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::{
    persist_recent_searches, sync_explore_shelves, sync_recent_searches, sync_shelves_and_queue,
    sync_window,
};
use crate::slint_shell::theme::{apply_theme, theme_from_index};
use crate::slint_shell::window::{
    open_detail_async, refresh_library_async, submit_search_async, trigger_playback,
};

/// Maps a clicked search result item to playback or container detail view.
pub(crate) fn handle_filtered_item_click<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
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
            trigger_playback(window, state, worker_tx);
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

/// Non-blocking handler for playback toggle (pause, resume, or start head track).
pub(crate) fn handle_toggle_play<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let audio_arc = std::sync::Arc::clone(&state.audio);
    let Ok(mut audio) = audio_arc.try_lock() else {
        return;
    };
    if audio.is_playing() {
        audio.pause();
        drop(audio);
        state.sync_presence_playing(false);
        window.set_is_playing(false);
        return;
    }
    if audio.last_requested().is_some() {
        audio.resume_play();
        drop(audio);
        state.sync_presence_playing(true);
        window.set_is_playing(true);
        return;
    }
    drop(audio);
    trigger_playback(window, state, worker_tx);
}

/// Wires user event callbacks between MoggytWindow and the application state without blocking.
pub(crate) fn wire_window_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: SharedState<C, A, S, L, K>,
    worker_tx: std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    wire_navigation_callbacks(window, &state_rc, &worker_tx);
    wire_search_callbacks(window, &state_rc, &worker_tx);
    wire_library_callbacks(window, &state_rc, &worker_tx);
    wire_detail_callbacks(window, &state_rc, &worker_tx);
    wire_playback_callbacks(window, &state_rc, &worker_tx);
    wire_settings_callbacks(window, &state_rc, &worker_tx);
}

/// Wires page navigation, back navigation, and queue panel drawer visibility callbacks.
fn wire_navigation_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
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
        if page == SlintPage::Library {
            refresh_library_async(&win, &mut st, &tx);
        } else if page == SlintPage::Explore && !st.explore_loaded && !st.explore_loading {
            load_explore_async(&win, &mut st, &tx);
        } else if page == SlintPage::Explore {
            refresh_explore_covers_if_loaded(&st, &tx);
        }
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
fn wire_search_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
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
            handle_filtered_item_click(&win, &mut st, item, &tx);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_filter_selected(move |filter_idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.search_filter = match filter_idx {
            1 => SearchFilter::Tracks,
            2 => SearchFilter::Albums,
            3 => SearchFilter::Artists,
            4 => SearchFilter::Playlists,
            _ => SearchFilter::All,
        };
        win.set_search_filter(filter_idx);
        sync_shelves_and_queue(&win, &st);
    });
}

/// Wires library container and queue selection callbacks.
fn wire_library_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_queue_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.play_track_at_index_in_memory(idx as usize);
        trigger_playback(&win, &mut st, &tx);
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

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_artist_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(a) = st.library_artists.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &a.id, DetailKind::Artist, &tx);
        }
    });
}

/// Wires detail view track selection and playback action callbacks.
fn wire_detail_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(detail) = st.detail.clone() {
            if let Some(track) = detail.tracks.get(idx as usize).cloned() {
                request_queue_track_cover(
                    track.artwork.as_deref(),
                    st.play_generation,
                    &st.search_covers,
                    &tx,
                );
                st.queue.push(track);
                let last = st.queue.len().saturating_sub(1);
                st.play_track_at_index_in_memory(last);
                trigger_playback(&win, &mut st, &tx);
            }
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_play(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(detail) = st.detail.clone() {
            if !detail.tracks.is_empty() {
                let start_idx = st.queue.len();
                for track in &detail.tracks {
                    request_queue_track_cover(
                        track.artwork.as_deref(),
                        st.play_generation,
                        &st.search_covers,
                        &tx,
                    );
                }
                for track in detail.tracks {
                    st.queue.push(track);
                }
                st.play_track_at_index_in_memory(start_idx);
                trigger_playback(&win, &mut st, &tx);
            }
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_shuffle(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(detail) = st.detail.clone() {
            if !detail.tracks.is_empty() {
                st.is_shuffle = true;
                let start_idx = st.queue.len();
                for track in &detail.tracks {
                    request_queue_track_cover(
                        track.artwork.as_deref(),
                        st.play_generation,
                        &st.search_covers,
                        &tx,
                    );
                }
                for track in detail.tracks {
                    st.queue.push(track);
                }
                st.play_track_at_index_in_memory(start_idx);
                trigger_playback(&win, &mut st, &tx);
            }
        }
        sync_window(&win, &mut st);
    });
}

/// Wires playback transport, track stepping, seek, volume, and preference callbacks.
fn wire_playback_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_toggle_play(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        handle_toggle_play(&win, &mut st, &tx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_prev_track(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.queue.retreat();
        trigger_playback(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_next_track(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.queue.advance();
        trigger_playback(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_seek_by(move |delta| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.seek_by(delta as i64);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_seek_to(move |secs| {
        let Some(win) = w.upgrade() else { return };
        let st = s.borrow();
        let target = secs.max(0) as u64;
        if let Ok(current) = st.audio.try_lock().map(|a| a.position_secs()) {
            if !should_seek(target, current) {
                return;
            }
        }
        let _ = tx.send(PlaybackWorkerCommand::Seek { position_secs: target });
        win.set_position_secs(target as i32);
        win.set_position_text(format_position(target as u32).into());
        let dur = st.queue.current().and_then(|t| t.duration_secs).unwrap_or(0);
        win.set_playback_progress(progress_fraction(target as u32, dur));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_set_volume(move |vol| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.set_volume(vol.clamp(0, 100) as u8);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_toggle_like(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(track) = st.queue.current() {
            let id = track.id.clone();
            let is_liked = toggle_like(&mut st.liked_ids, &id);
            win.set_is_liked(is_liked);
        }
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_toggle_shuffle(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.is_shuffle = !st.is_shuffle;
        win.set_is_shuffle(st.is_shuffle);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_cycle_repeat(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let next = cycle_repeat_mode(st.queue.repeat());
        st.queue.set_repeat(next);
        win.set_repeat_mode(repeat_mode_to_int(next));
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_cycle_playback_speed(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let speed = st.cycle_playback_speed();
        win.set_playback_speed_text(format_playback_speed(speed).into());
    });
}

/// Wires settings, account authentication, player bar preferences, and resize callbacks.
fn wire_settings_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: &SharedState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_sign_in_submitted(move |cookies| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if st.sign_in_from_input(cookies.as_str()) {
            win.set_cookie_text("".into());
            win.set_settings_cookie_text("".into());
            refresh_library_async(&win, &mut st, &tx);
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_sign_out(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.sign_out();
        refresh_library_async(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_theme_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let theme = theme_from_index(idx);
        st.set_theme(theme);
        apply_theme(&win, theme);
        win.set_active_theme_index(idx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_player_bar_mode_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let mode = mode_from_index(idx);
        st.set_player_bar_mode(mode);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_player_bar_option_toggled(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.toggle_player_bar_option(idx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_width_changed(move |width| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let columns = explore_columns_for_width(width);
        if st.explore_columns == columns {
            return;
        }
        st.explore_columns = columns;
        if st.explore_loaded {
            sync_explore_shelves(&win, &st);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Detail, Track};
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
}
