//! Window composition, UI event loop wiring, background worker threads, and timer.
//!
//! Hexagonal invariant: adapter composition root connecting Slint UI events and callbacks
//! to application state without direct domain business rules.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::sync::set_lyrics_highlight;
use crate::domain::{Detail, DetailKind, SearchResult, Shelf, is_local_playlist_id};
use crate::i18n::tr;
use crate::infra::{
    AudioBackend, CatalogError, CoverCache, DiskPlaylistStore, LyricsSource, PlaylistStore,
    default_config_dir,
};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::callbacks::wire_window_callbacks;
use crate::slint_shell::covers::{
    request_detail_covers, request_library_covers, request_shelf_covers, resolve_row_cover_image,
    spawn_cover_fetch_pool,
};
use crate::slint_shell::format::format_queue_label;
use crate::slint_shell::history::{is_history_id, label_history_detail, with_history_entry};
use crate::slint_shell::import_callbacks::{
    current_store, install_import_callbacks, install_playlist_store, poll_import,
};
use crate::slint_shell::import_state::{local_detail, merge_local_library};
use crate::slint_shell::init::apply_updater_release;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback::{format_track_duration, is_track_liked};
use crate::slint_shell::playback_control::apply_play_track_completion;
use crate::slint_shell::playback_worker::{
    PlaybackWorkerAdapters, PlaybackWorkerCommand, execute_detail, execute_play_track,
    execute_refresh_library, execute_seek, spawn_lyrics_prefetch, thumbnail_variant_for_row,
};
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::stream_prefetch::spawn_stream_prefetch;
use crate::slint_shell::sync::{
    observe_view_phases, sync_header_title, sync_library_and_detail, sync_playback_tick,
    sync_recent_searches, sync_shelves_and_queue, sync_update_status, sync_view_phase, sync_window,
};
use crate::slint_shell::theme::apply_custom_theme_if_changed;
use crate::slint_shell::view_phase::{ViewId, ViewPhase};

thread_local! {
    static ACTIVE_STATE: RefCell<Option<SharedState>> = const { RefCell::new(None) };
}

pub(crate) fn set_ui_state(state_rc: &SharedState) {
    ACTIVE_STATE.with(|cell| {
        *cell.borrow_mut() = Some(Rc::clone(state_rc));
    });
}

/// Runs `f` with the installed app state, if any.
///
/// Returns `None` when no state is installed and also when the state is already
/// mutably borrowed, which happens when a callback or timer re-enters this
/// helper from inside a borrow. Callers treat `None` as "skip this sync": a
/// re-entrant UI callback must degrade, never abort the process.
pub(crate) fn with_ui_state<R>(f: impl FnOnce(&mut SlintAppState) -> R) -> Option<R> {
    ACTIVE_STATE.with(|cell| {
        let borrow = cell.borrow();
        let rc = borrow.as_ref()?;
        let mut st = rc.try_borrow_mut().ok()?;
        Some(f(&mut st))
    })
}

/// Triggers asynchronous track playback on the background worker thread.
pub(crate) fn trigger_playback(window: &MoggytWindow, state: &mut SlintAppState) {
    let Some(req) = state.begin_play_request() else {
        return;
    };
    let Some(track) = state.queue.current().cloned() else {
        return;
    };
    let generation = req.generation;

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
            if let Some(ref tx) = state.worker_tx {
                let _ = tx.send(PlaybackWorkerCommand::FetchCover { url: target_url, generation });
            }
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
    set_lyrics_highlight(window, String::new(), String::new());
    state.send_play_request(req);
}

pub(crate) use super::search_submit::{submit_search_async, submit_search_with_filter_async};

/// Exits search mode from the UI and resyncs the search view.
pub(crate) fn clear_search_and_sync(window: &MoggytWindow, state: &mut SlintAppState) {
    state.clear_search();
    window.set_query("".into());
    sync_recent_searches(window, state);
    sync_shelves_and_queue(window, state);
    observe_view_phases(state);
    sync_view_phase(window, ViewId::Search, state.search_phase);
}

/// Resolves a local playlist detail model synchronously from the store if the id is local.
pub fn resolve_local_detail(browse_id: &str, store: &dyn PlaylistStore) -> Option<Detail> {
    let trimmed = browse_id.trim();
    if !is_local_playlist_id(trimmed) {
        return None;
    }
    store.get(trimmed).map(|p| local_detail(&p))
}

fn apply_detail_result(
    detail: Option<Detail>,
    id: &str,
    locale: crate::i18n::Locale,
) -> Option<Detail> {
    if is_history_id(id) { detail.map(|d| label_history_detail(d, locale)) } else { detail }
}

fn begin_detail_load(state: &mut SlintAppState, browse_id: &str, kind: DetailKind) -> Option<u64> {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if state.page != crate::slint_shell::page::SlintPage::Detail {
        state.detail_return = state.page;
    }
    state.page = crate::slint_shell::page::SlintPage::Detail;
    state.detail_generation = state.detail_generation.wrapping_add(1);
    state.detail_phase = ViewPhase::Loading;
    state.detail_error = None;
    state.detail_request = Some((trimmed.to_string(), kind));
    Some(state.detail_generation)
}

pub(crate) fn apply_async_search_outcome(
    state: &mut SlintAppState,
    generation: u64,
    query: String,
    shelves: Result<Vec<Shelf>, CatalogError>,
) -> bool {
    if state.search_generation != generation {
        return false;
    }
    state.search_query = query;
    match shelves {
        Ok(shelves) => {
            let is_empty = shelves.iter().all(|s| s.items.is_empty());
            state.home_shelves = shelves;
            state.search_results = state
                .home_shelves
                .iter()
                .flat_map(|s| s.items.iter())
                .filter_map(|it| match it {
                    SearchResult::Track(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
            state.search_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
            true
        }
        Err(_) => {
            state.search_phase = ViewPhase::Error;
            false
        }
    }
}

fn apply_detail_outcome(
    state: &mut SlintAppState,
    generation: u64,
    detail: Result<Option<Detail>, CatalogError>,
    id: &str,
) -> bool {
    if state.detail_generation != generation {
        return false;
    }
    match detail {
        Ok(Some(detail)) => {
            let is_empty = detail.tracks.is_empty();
            state.detail = apply_detail_result(Some(detail), id, state.locale);
            state.detail_error = None;
            state.detail_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
        }
        Ok(None) => {
            state.detail = None;
            state.detail_error = None;
            state.detail_phase = ViewPhase::Empty;
        }
        Err(_) => {
            state.detail = None;
            state.detail_phase = ViewPhase::Error;
            state.detail_error = Some("Failed to load detail".to_string());
        }
    }
    true
}

/// Dispatches asynchronous container detail lookup off the UI thread.
pub(crate) fn open_detail_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    browse_id: &str,
    kind: DetailKind,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(generation) = begin_detail_load(state, browse_id, kind) else {
        return;
    };
    let trimmed = browse_id.trim();
    if is_local_playlist_id(trimmed) {
        let detail = current_store().and_then(|st| resolve_local_detail(trimmed, st.as_ref()));
        apply_detail_outcome(state, generation, Ok(detail), trimmed);
        sync_window(window, state);
        return;
    }
    state.detail = Some(Detail::new(kind, tr(state.locale, "Loading..."), "", Vec::new()));
    sync_window(window, state);

    let id = trimmed.to_string();
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();

    std::thread::spawn(move || {
        let (g, detail) = execute_detail(generation, &id, kind, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state(|st| {
                if apply_detail_outcome(st, g, detail, &id) {
                    request_detail_covers(st, &tx);
                    sync_library_and_detail(&win, st);
                    sync_header_title(&win, st);
                }
            });
        });
    });
}

/// Re-dispatches pending or failed detail request using captured parameters.
pub fn retry_detail_load(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some((id, kind)) = state.detail_request.clone() else {
        return;
    };
    open_detail_async(window, state, &id, kind, worker_tx);
}

fn is_library_empty(p: &[crate::domain::Playlist], a: &[crate::domain::Album]) -> bool {
    p.is_empty() && a.is_empty()
}

fn apply_async_library_outcome(
    state: &mut SlintAppState,
    generation: u64,
    playlists: Result<Vec<crate::domain::Playlist>, CatalogError>,
    albums: Result<Vec<crate::domain::Album>, CatalogError>,
) -> bool {
    if state.library_generation != generation {
        return false;
    }
    match (playlists, albums) {
        (Ok(playlists), Ok(albums)) => {
            let local = current_store().map(|s| s.list()).unwrap_or_default();
            let is_empty = is_library_empty(&playlists, &albums);
            state.library_playlists = with_history_entry(
                merge_local_library(&playlists, &local),
                state.signed_in,
                state.locale,
            );
            state.library_albums = albums;
            state.library_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
            true
        }
        _ => {
            state.library_phase = ViewPhase::Error;
            false
        }
    }
}

/// Dispatches asynchronous library refresh off the UI thread.
pub(crate) fn refresh_library_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
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
            with_ui_state(|st| {
                if apply_async_library_outcome(st, g, playlists, albums) {
                    request_library_covers(st, &tx);
                }
                sync_library_and_detail(&win, st);
            });
        });
    });
}

/// Spawns the dedicated playback worker thread processing play and scrobble commands.
pub(crate) fn spawn_playback_worker(
    window: &MoggytWindow,
    adapters: PlaybackWorkerAdapters<
        Box<dyn AudioBackend + Send>,
        Box<dyn LyricsSource + Send + Sync>,
        Box<dyn CoverCache + Send>,
    >,
    rx: std::sync::mpsc::Receiver<PlaybackWorkerCommand>,
) {
    let weak = window.as_weak();
    let cover_tx = spawn_cover_fetch_pool(Arc::clone(&adapters.covers), weak.clone());

    std::thread::Builder::new()
        .name("moggyt-playback".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    PlaybackWorkerCommand::Play(req) => {
                        let prefetch = req.prefetch_lyrics.clone();
                        let stream_id = req.prefetch_stream_id.clone();
                        let res = execute_play_track(req, &adapters);
                        let w = weak.clone();
                        let _ = dispatch_to_ui(move || {
                            let Some(win) = w.upgrade() else { return };
                            with_ui_state(|st| {
                                if st.should_apply_play_result(res.generation) {
                                    apply_play_track_completion(&win, st, res);
                                }
                            });
                        });
                        spawn_lyrics_prefetch(prefetch, Arc::clone(&adapters.lyrics));
                        spawn_stream_prefetch(stream_id, &adapters.audio);
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
pub fn build_window(state: SlintAppState) -> anyhow::Result<(MoggytWindow, slint::Timer)> {
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
    spawn_playback_worker(&window, adapters, worker_rx);
    request_shelf_covers(&state_rc.borrow(), &worker_tx);

    state_rc.borrow_mut().worker_tx = Some(worker_tx.clone());
    sync_window(&window, &mut state_rc.borrow_mut());
    window.set_query(state_rc.borrow().search_query.as_str().into());
    wire_window_callbacks(&window, Rc::clone(&state_rc), worker_tx);
    install_playlist_store(Arc::new(DiskPlaylistStore::new(default_config_dir())));
    install_import_callbacks(&window);
    let timer = setup_app_timer(&window, Rc::clone(&state_rc));

    Ok((window, timer))
}

/// Sets up periodic background timer for tray events, media keys, community skip polling, auto-skip, and tick sync.
pub(crate) fn setup_app_timer(window: &MoggytWindow, state_rc: SharedState) -> slint::Timer {
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(500), move || {
        let Some(win) = weak.upgrade() else { return };
        // The import poll re-enters the app state through `with_ui_state`, so it
        // must run before this borrow is taken.
        poll_import(&win);
        let mut state = state_rc.borrow_mut();
        if state.drain_tray_actions() {
            let _ = state.queue_store.save(&state.queue);
            slint::quit_event_loop().ok();
            return;
        }
        state.drain_media_keys();
        state.poll_sponsor_ranges();
        state.check_auto_skip();
        state.continue_autoplay_if_ended();
        state.poll_sleep_timer();
        apply_custom_theme_if_changed(&win, &mut state);
        drain_updater_check(&win, &mut state);
        sync_playback_tick(&win, &mut state);
    });
    timer
}

/// Drains pending update check result without blocking, updating window properties if ready.
pub(crate) fn drain_updater_check(window: &MoggytWindow, state: &mut SlintAppState) {
    if poll_updater_check(state) {
        sync_update_status(window, state);
    }
}

/// Polls pending update check channel, applying any received release info to state.
pub(crate) fn poll_updater_check(state: &mut SlintAppState) -> bool {
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
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_set_and_with_ui_state_round_trip() {
        let state = test_wired();
        let rc = Rc::new(RefCell::new(state));
        set_ui_state(&rc);

        let initial_len = with_ui_state(|st: &mut SlintAppState| {
            st.queue.push(Track::new("t1", "Track One", "Artist One"));
            st.queue.len()
        });
        assert_eq!(initial_len, Some(1));

        let updated_len = with_ui_state(|st: &mut SlintAppState| st.queue.len());
        assert_eq!(updated_len, Some(1));
    }

    #[test]
    fn test_with_ui_state_passes_through_closure_return_value() {
        let state = test_wired();
        let rc = Rc::new(RefCell::new(state));
        set_ui_state(&rc);

        let result = with_ui_state(|st: &mut SlintAppState| {
            st.is_shuffle = true;
            st.is_shuffle
        });
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_with_ui_state_returns_none_when_already_borrowed() {
        let state_rc: SharedState = Rc::new(RefCell::new(test_wired()));
        set_ui_state(&state_rc);
        let _held = state_rc.borrow_mut();
        assert!(with_ui_state(|st: &mut SlintAppState| st.queue.len()).is_none());
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
        assert!(poll_updater_check(&mut state));
        assert!(state.update_available);
        assert_eq!(state.latest_version.as_deref(), Some("v99.0.0"));
        assert!(state.updater_rx.is_none());
        assert!(!poll_updater_check(&mut state));
    }

    #[test]
    fn test_polling_disconnected_channel_clears_receiver_without_modifying_state() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel::<Option<crate::infra::ReleaseInfo>>();
        state.updater_rx = Some(rx);
        drop(tx);

        assert!(!poll_updater_check(&mut state));
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);
        assert!(state.updater_rx.is_none());
    }

    #[test]
    fn test_resolve_local_detail_resolves_stored_playlist() {
        let store = crate::infra::MemoryPlaylistStore::new();
        let mut pl = crate::domain::LocalPlaylist::new("local:road_trip", "Road Trip");
        pl.tracks.push(crate::domain::LocalTrack::new(
            "v1",
            "Song One",
            "Artist One",
            crate::domain::MatchConfidence::High,
        ));
        store.save(&pl).unwrap();

        let detail = resolve_local_detail("local:road_trip", &store)
            .expect("resolves local playlist detail");
        assert_eq!(detail.title, "Road Trip");
        assert_eq!(detail.kind, DetailKind::Playlist);
        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(detail.tracks[0].id, "v1");
    }

    #[test]
    fn test_resolve_local_detail_unknown_or_non_local_yields_none() {
        let store = crate::infra::MemoryPlaylistStore::new();
        assert!(resolve_local_detail("local:missing_id", &store).is_none());
        assert!(resolve_local_detail("spotify:non_local", &store).is_none());
    }

    #[test]
    fn local_detail_after_remote_keeps_local() {
        let mut state = test_wired();
        let r_gen = begin_detail_load(&mut state, "album-1", DetailKind::Album).unwrap();
        let l_gen = begin_detail_load(&mut state, "local:race", DetailKind::Playlist).unwrap();
        assert!(l_gen > r_gen);

        let local =
            Detail::new(DetailKind::Playlist, "Local", "", vec![Track::new("t1", "T", "A")]);
        assert!(apply_detail_outcome(&mut state, l_gen, Ok(Some(local)), "local:race"));
        assert_eq!(state.detail_phase, ViewPhase::Ready);
        assert_eq!(state.detail.as_ref().map(|d| d.title.as_str()), Some("Local"));

        let remote =
            Detail::new(DetailKind::Album, "Remote", "", vec![Track::new("t2", "T2", "A2")]);
        assert!(!apply_detail_outcome(&mut state, r_gen, Ok(Some(remote)), "album-1"));
        assert_eq!(state.detail_phase, ViewPhase::Ready);
        assert_eq!(state.detail.as_ref().map(|d| d.title.as_str()), Some("Local"));
    }

    #[test]
    fn detail_failure_sets_error_phase() {
        let mut state = test_wired();
        let generation =
            begin_detail_load(&mut state, "album-fail", DetailKind::Album).expect("load begins");
        let applied = apply_detail_outcome(
            &mut state,
            generation,
            Err(CatalogError::Transport),
            "album-fail",
        );
        assert!(applied);
        assert_eq!(state.detail_phase, ViewPhase::Error);
        assert!(state.detail_error.is_some());
        assert!(state.detail.is_none());
        assert_eq!(state.detail_request, Some(("album-fail".to_string(), DetailKind::Album)));
    }

    #[test]
    fn detail_not_found_is_empty_not_error() {
        let mut state = test_wired();
        let generation =
            begin_detail_load(&mut state, "album-404", DetailKind::Album).expect("load begins");
        assert!(apply_detail_outcome(&mut state, generation, Ok(None), "album-404"));
        assert_eq!(state.detail_phase, ViewPhase::Empty);
        assert_eq!(state.detail_error, None);
        assert!(state.detail.is_none());
    }

    #[test]
    fn local_detail_bumps_generation() {
        let mut state = test_wired();
        let gen1 = begin_detail_load(&mut state, "local:pl1", DetailKind::Playlist).unwrap();
        assert_eq!(state.detail_generation, gen1);
        assert_eq!(state.detail_phase, ViewPhase::Loading);

        let gen2 = begin_detail_load(&mut state, "local:pl2", DetailKind::Playlist).unwrap();
        assert_eq!(gen2, gen1.wrapping_add(1));
        assert_eq!(state.detail_generation, gen2);
        assert_eq!(state.detail_phase, ViewPhase::Loading);
    }

    #[test]
    fn empty_detail_is_empty_not_error() {
        let mut state = test_wired();
        let generation =
            begin_detail_load(&mut state, "album-empty", DetailKind::Album).expect("load begins");
        let empty_detail = Detail::new(DetailKind::Album, "Empty Album", "", Vec::new());
        let applied =
            apply_detail_outcome(&mut state, generation, Ok(Some(empty_detail)), "album-empty");
        assert!(applied);
        assert_eq!(state.detail_phase, ViewPhase::Empty);
        assert_eq!(state.detail_error, None);
        assert!(state.detail.is_some());
    }

    #[test]
    fn async_search_outcome_handles_error_and_empty() {
        let mut state = test_wired();
        state.search_generation = 10;
        let initial_shelves = vec![Shelf::new("Initial")];
        state.home_shelves = initial_shelves.clone();

        let applied_stale =
            apply_async_search_outcome(&mut state, 9, "rock".into(), Err(CatalogError::Transport));
        assert!(!applied_stale);

        let applied_err =
            apply_async_search_outcome(&mut state, 10, "rock".into(), Err(CatalogError::Transport));
        assert!(!applied_err);
        assert_eq!(state.search_phase, ViewPhase::Error);
        assert_eq!(state.home_shelves, initial_shelves);

        let applied_ok = apply_async_search_outcome(&mut state, 10, "rock".into(), Ok(Vec::new()));
        assert!(applied_ok);
        assert_eq!(state.search_phase, ViewPhase::Empty);
        assert!(state.home_shelves.is_empty());
    }

    #[test]
    fn async_library_outcome_handles_error_and_empty() {
        let mut state = test_wired();
        state.library_generation = 10;
        let applied_stale = apply_async_library_outcome(
            &mut state,
            9,
            Err(CatalogError::Transport),
            Err(CatalogError::Transport),
        );
        assert!(!applied_stale);

        let initial = crate::domain::Playlist::new("p1", "Initial", 1, None);
        state.library_playlists = vec![initial.clone()];
        let applied_err = apply_async_library_outcome(
            &mut state,
            10,
            Err(CatalogError::Transport),
            Err(CatalogError::Transport),
        );
        assert!(!applied_err);
        assert_eq!(state.library_phase, ViewPhase::Error);
        assert_eq!(state.library_playlists, vec![initial]);

        assert!(apply_async_library_outcome(&mut state, 10, Ok(Vec::new()), Ok(Vec::new())));
        assert_eq!(state.library_phase, ViewPhase::Empty);
    }

    #[test]
    fn clearing_query_ignores_in_flight_results() {
        let mut state = test_wired();
        state.search_generation = 7;
        state.search_active = true;
        state.search_query = "rock".into();
        state.search_phase = ViewPhase::Loading;

        state.clear_search();
        let applied = apply_async_search_outcome(&mut state, 7, "rock".into(), Ok(Vec::new()));

        assert!(!applied);
        assert!(!state.search_active);
        assert_eq!(state.search_query, "");
        assert_eq!(state.search_phase, ViewPhase::Idle);
    }
}
