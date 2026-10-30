//! Window composition, UI event loop wiring, background worker threads, and timer.
//!
//! Hexagonal invariant: adapter composition root connecting Slint UI events and callbacks
//! to application state without direct domain business rules.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{Detail, DetailKind, SearchResult, Shelf};
use crate::i18n::tr;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::callbacks::wire_window_callbacks;
use crate::slint_shell::covers::{
    request_detail_covers, request_library_covers, request_shelf_covers, resolve_row_cover_image,
    spawn_cover_fetch_pool,
};
use crate::slint_shell::format::format_queue_label;
use crate::slint_shell::init::apply_updater_release;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback::{format_track_duration, is_track_liked};
use crate::slint_shell::playback_control::apply_play_track_completion;
use crate::slint_shell::playback_worker::{
    PlayTrackRequest, PlaybackWorkerAdapters, PlaybackWorkerCommand, execute_detail,
    execute_play_track, execute_refresh_library, execute_scrobble, execute_search, execute_seek,
    thumbnail_variant_for_row,
};
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::{
    persist_recent_searches, sync_library_and_detail, sync_playback_tick, sync_recent_searches,
    sync_shelves_and_queue, sync_update_status, sync_window,
};

thread_local! {
    static ACTIVE_STATE: RefCell<Option<Box<dyn Any>>> = RefCell::new(None);
}

pub(crate) fn set_ui_state<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state_rc: &SharedState<C, A, S, L, K>,
) {
    ACTIVE_STATE.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(Rc::clone(state_rc)));
    });
}

pub(crate) fn with_ui_state<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
    R,
>(
    f: impl FnOnce(&mut SlintAppState<C, A, S, L, K>) -> R,
) -> Option<R> {
    ACTIVE_STATE.with(|cell| {
        let borrow = cell.borrow();
        let any = borrow.as_ref()?;
        let rc = any.downcast_ref::<SharedState<C, A, S, L, K>>()?;
        let mut st = rc.borrow_mut();
        Some(f(&mut st))
    })
}

/// Triggers asynchronous track playback on the background worker thread.
pub(crate) fn trigger_playback<
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
    let Some(track) = state.queue.current().cloned() else {
        return;
    };
    state.play_generation = state.play_generation.wrapping_add(1);
    let generation = state.play_generation;
    state.maybe_scrobble_current();

    state.current_cover_image = None;
    let initial_cover = resolve_row_cover_image(track.artwork.as_deref(), &state.search_covers);
    if initial_cover.size().width > 0 {
        state.now_playing_cover = Some(initial_cover.clone());
        window.set_now_playing_cover(initial_cover);
    } else {
        state.now_playing_cover = None;
        window.set_now_playing_cover(slint::Image::default());
    }

    if let Some(ref art_url) = track.artwork {
        let target_url =
            if state.hi_res_covers { art_url.clone() } else { thumbnail_variant_for_row(art_url) };
        if !state.search_covers.contains_key(&target_url) {
            let _ =
                worker_tx.send(PlaybackWorkerCommand::FetchCover { url: target_url, generation });
        }
    }
    state.lyrics_lines.clear();
    state.lyrics_key = None;

    window.set_is_playing(true);
    window.set_active_track_title(track.title.clone().into());
    window.set_active_track_artist(track.artist.clone().into());
    let dur_secs = track.duration_secs.unwrap_or(0);
    window.set_duration_text(format_track_duration(track.duration_secs).into());
    window.set_duration_secs(dur_secs as i32);
    window.set_position_text("0:00".into());
    window.set_position_secs(0);
    window.set_playback_progress(0.0);
    window.set_is_liked(is_track_liked(&state.liked_ids, &track.id));
    window.set_queue_count(state.queue.len() as i32);
    window.set_queue_button_text(format_queue_label(state.locale, state.queue.len()).into());
    window.set_has_cover(false);
    window.set_lyrics_lines(ModelRc::new(VecModel::default()));
    window.set_active_lyric_index(-1);

    let req = PlayTrackRequest {
        generation,
        track_id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        artwork_url: track.artwork.clone(),
        duration_secs: track.duration_secs,
        hi_res_covers: state.hi_res_covers,
        lyric_overrides: state.lyric_overrides.clone(),
    };
    let _ = worker_tx.send(PlaybackWorkerCommand::Play(req));
}

/// Dispatches asynchronous search query off the UI thread.
pub(crate) fn submit_search_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    query: &str,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return;
    }
    state.search_active = true;
    state.recent_searches.record(trimmed);
    persist_recent_searches(state);
    sync_recent_searches(window, state);

    state.search_query = query.to_string();
    state.search_filter = crate::domain::SearchFilter::All;
    state.search_covers.clear();
    state.search_generation = state.search_generation.wrapping_add(1);
    let generation = state.search_generation;

    window.set_query(query.into());
    window.set_search_filter(0);
    state.home_shelves = vec![Shelf::new(tr(state.locale, "Searching..."))];
    sync_shelves_and_queue(window, state);

    let cat = Arc::clone(&state.catalog);
    let q = query.to_string();
    let weak = window.as_weak();
    let tx = worker_tx.clone();
    let loc = state.locale;

    std::thread::spawn(move || {
        let (g, q_res, shelves) = execute_search(generation, &q, &cat, loc);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.search_generation == g {
                    st.search_query = q_res;
                    st.home_shelves = shelves;
                    st.search_results = st
                        .home_shelves
                        .iter()
                        .flat_map(|s| s.items.iter())
                        .filter_map(|it| match it {
                            SearchResult::Track(t) => Some(t.clone()),
                            _ => None,
                        })
                        .collect();
                    request_shelf_covers(st, &tx);
                    win.set_query(st.search_query.as_str().into());
                    sync_shelves_and_queue(&win, st);
                }
            });
        });
    });
}

/// Dispatches asynchronous container detail lookup off the UI thread.
pub(crate) fn open_detail_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    browse_id: &str,
    kind: DetailKind,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return;
    }
    if state.page != crate::slint_shell::page::SlintPage::Detail {
        state.detail_return = state.page;
    }
    state.page = crate::slint_shell::page::SlintPage::Detail;
    state.detail = Some(Detail::new(kind, tr(state.locale, "Loading..."), "", Vec::new()));
    sync_window(window, state);

    state.detail_generation = state.detail_generation.wrapping_add(1);
    let generation = state.detail_generation;
    let id = trimmed.to_string();
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();

    std::thread::spawn(move || {
        let (g, detail) = execute_detail(generation, &id, kind, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.detail_generation == g {
                    st.detail = detail;
                    request_detail_covers(st, &tx);
                    sync_library_and_detail(&win, st);
                }
            });
        });
    });
}

/// Dispatches asynchronous library refresh off the UI thread.
pub(crate) fn refresh_library_async<
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
    state.library_generation = state.library_generation.wrapping_add(1);
    let generation = state.library_generation;
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();

    std::thread::spawn(move || {
        let (g, playlists, albums) = execute_refresh_library(generation, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.library_generation == g {
                    st.library_playlists = playlists;
                    st.library_albums = albums;
                    st.library_artists = Vec::new();
                    request_library_covers(st, &tx);
                    sync_library_and_detail(&win, st);
                }
            });
        });
    });
}

/// Spawns the dedicated playback worker thread processing play and scrobble commands.
pub(crate) fn spawn_playback_worker<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    adapters: PlaybackWorkerAdapters<A, L, K>,
    rx: std::sync::mpsc::Receiver<PlaybackWorkerCommand>,
) {
    let weak = window.as_weak();
    let cover_tx =
        spawn_cover_fetch_pool::<C, A, S, L, K>(Arc::clone(&adapters.covers), weak.clone());

    std::thread::Builder::new()
        .name("moggyt-playback".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    PlaybackWorkerCommand::Play(req) => {
                        let res = execute_play_track(req, &adapters);
                        let w = weak.clone();
                        let _ = dispatch_to_ui(move || {
                            let Some(win) = w.upgrade() else { return };
                            with_ui_state::<C, A, S, L, K, _>(|st| {
                                if st.play_generation == res.generation {
                                    apply_play_track_completion(&win, st, res);
                                }
                            });
                        });
                    }
                    PlaybackWorkerCommand::Scrobble { artist, title, started_unix } => {
                        execute_scrobble(
                            &artist,
                            &title,
                            started_unix,
                            &adapters.scrobbler,
                            &adapters.secondary_scrobbler,
                        );
                    }
                    PlaybackWorkerCommand::FetchCover { url, generation } => {
                        let _ = cover_tx.send((url, generation));
                    }
                    PlaybackWorkerCommand::Seek { position_secs } => {
                        execute_seek(position_secs, &adapters.audio);
                    }
                }
            }
        })
        .expect("failed to spawn playback worker thread");
}

/// Builds and configures MoggytWindow wired to SlintAppState with callbacks and background timer.
pub fn build_window<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: SlintAppState<C, A, S, L, K>,
) -> anyhow::Result<(MoggytWindow, slint::Timer)> {
    let window = MoggytWindow::new().context("failed to instantiate MoggytWindow")?;
    let state_rc = Rc::new(RefCell::new(state));
    set_ui_state(&state_rc);

    let adapters = PlaybackWorkerAdapters::new(
        Arc::clone(&state_rc.borrow().audio),
        Arc::clone(&state_rc.borrow().scrobbler),
        state_rc.borrow().secondary_scrobbler.clone(),
        Arc::clone(&state_rc.borrow().lyrics),
        Arc::clone(&state_rc.borrow().covers),
    );

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
    spawn_playback_worker::<C, A, S, L, K>(&window, adapters, worker_rx);
    request_shelf_covers(&state_rc.borrow(), &worker_tx);

    state_rc.borrow_mut().worker_tx = Some(worker_tx.clone());
    sync_window(&window, &mut state_rc.borrow_mut());
    window.set_query(state_rc.borrow().search_query.as_str().into());
    wire_window_callbacks(&window, Rc::clone(&state_rc), worker_tx);
    let timer = setup_app_timer(&window, Rc::clone(&state_rc));

    Ok((window, timer))
}

/// Sets up periodic background timer for tray events, media keys, auto-skip, and tick sync.
pub(crate) fn setup_app_timer<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: SharedState<C, A, S, L, K>,
) -> slint::Timer {
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(500), move || {
        let Some(win) = weak.upgrade() else { return };
        let mut state = state_rc.borrow_mut();
        if state.drain_tray_actions() {
            let _ = state.queue_store.save(&state.queue);
            slint::quit_event_loop().ok();
            return;
        }
        state.drain_media_keys();
        state.check_auto_skip();
        state.continue_autoplay_if_ended();
        state.poll_sleep_timer();
        drain_updater_check(&win, &mut state);
        sync_playback_tick(&win, &mut state);
    });
    timer
}

/// Drains pending update check result without blocking, updating window properties if ready.
pub(crate) fn drain_updater_check<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    if poll_updater_check(state) {
        sync_update_status(window, state);
    }
}

/// Polls pending update check channel, applying any received release info to state.
pub(crate) fn poll_updater_check<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &mut SlintAppState<C, A, S, L, K>,
) -> bool {
    let Some(ref rx) = state.updater_rx else {
        return false;
    };
    match rx.try_recv() {
        Ok(info) => {
            state.updater_rx = None;
            apply_updater_release(state, info);
            true
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            state.updater_rx = None;
            false
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::domain::Track;
    use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_set_and_with_ui_state_round_trip() {
        let state = test_wired();
        let rc = Rc::new(RefCell::new(state));
        set_ui_state(&rc);

        let initial_len = with_ui_state(
            |st: &mut SlintAppState<
                CatalogStub,
                AudioStub,
                MemorySessionStore,
                LrclibStub,
                MemoryCoverCache,
            >| {
                st.queue.push(Track::new("t1", "Track One", "Artist One"));
                st.queue.len()
            },
        );
        assert_eq!(initial_len, Some(1));

        let updated_len = with_ui_state(
            |st: &mut SlintAppState<
                CatalogStub,
                AudioStub,
                MemorySessionStore,
                LrclibStub,
                MemoryCoverCache,
            >| { st.queue.len() },
        );
        assert_eq!(updated_len, Some(1));
    }

    #[test]
    fn test_with_ui_state_passes_through_closure_return_value() {
        let state = test_wired();
        let rc = Rc::new(RefCell::new(state));
        set_ui_state(&rc);

        let result = with_ui_state(
            |st: &mut SlintAppState<
                CatalogStub,
                AudioStub,
                MemorySessionStore,
                LrclibStub,
                MemoryCoverCache,
            >| {
                st.is_shuffle = true;
                st.is_shuffle
            },
        );
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_polling_applies_exactly_one_queued_result_and_leaves_state_unchanged() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel();
        state.updater_rx = Some(rx);

        tx.send(Some(crate::infra::ReleaseInfo {
            version: "v99.0.0".to_string(),
            url: "https://github.com/example/releases/tag/v99.0.0".to_string(),
        }))
        .unwrap();

        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);
        assert!(state.updater_rx.is_some());

        // First poll: applies the queued result and clears the channel
        let applied = poll_updater_check(&mut state);
        assert!(applied);
        assert!(state.update_available);
        assert_eq!(state.latest_version.as_deref(), Some("v99.0.0"));
        assert!(state.updater_rx.is_none());

        // Subsequent poll: state unchanged
        let second_poll = poll_updater_check(&mut state);
        assert!(!second_poll);
        assert!(state.update_available);
        assert_eq!(state.latest_version.as_deref(), Some("v99.0.0"));
        assert!(state.updater_rx.is_none());
    }

    #[test]
    fn test_polling_disconnected_channel_clears_receiver_without_modifying_state() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel::<Option<crate::infra::ReleaseInfo>>();
        state.updater_rx = Some(rx);
        drop(tx);

        let applied = poll_updater_check(&mut state);
        assert!(!applied);
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);
        assert!(state.updater_rx.is_none());
    }
}
