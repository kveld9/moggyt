//! Slint event loop bindings and worker thread wiring for Spotify playlist imports.
//!
//! Hexagonal invariant: adapter connecting Slint UI callbacks and background import
//! pipeline threads to the in-memory session state machine without direct domain rules.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{SpotifyPlaylist, Track, parse_spotify_ref};
use crate::i18n::{Locale, tr};
use crate::infra::{
    FilePicker, KeyringSpotifyCredentialStore, PlaylistStore, RfdFilePicker,
    SpotifyCredentialStore, load_playlist_file, load_spotify_link,
};
use crate::slint_shell::import_searcher::LiveTrackSearcher;
use crate::slint_shell::import_session::{ImportDestination, ImportSession, ImportStage};
use crate::slint_shell::import_state::{
    ImportCounts, ImportReview, alternatives_to_slint, counts_text, rows_to_slint,
};
use crate::slint_shell::import_worker::{WorkerConfig, spawn_import};
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{ImportUi, MoggytWindow};

static SESSION: Mutex<Option<ImportSession>> = Mutex::new(None);
static STORE: Mutex<Option<Arc<dyn PlaylistStore>>> = Mutex::new(None);

/// Pure helper evaluating Spotify input link text against supported link patterns.
pub(crate) fn validate_import_link(text: &str, locale: Locale) -> (bool, String) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (false, String::new());
    }
    if parse_spotify_ref(trimmed).is_some() {
        (true, tr(locale, "Valid Spotify link").to_string())
    } else {
        (false, tr(locale, "Invalid Spotify link").to_string())
    }
}

/// Appends imported tracks to the playback queue and returns the index of the first added track.
pub(crate) fn enqueue_imported_tracks(st: &mut SlintAppState, tracks: Vec<Track>) -> usize {
    let start_idx = st.queue.len();
    for track in tracks {
        st.queue.push(track);
    }
    start_idx
}

/// Installs the shared playlist persistence store.
pub fn install_playlist_store(store: Arc<dyn PlaylistStore>) {
    if let Ok(mut guard) = STORE.lock() {
        *guard = Some(store);
    }
}

/// Returns a clone of the installed playlist store, if any.
pub fn current_store() -> Option<Arc<dyn PlaylistStore>> {
    STORE.lock().ok()?.clone()
}

/// Executes a closure mutating the active import session under lock.
pub fn with_session<R>(f: impl FnOnce(&mut ImportSession) -> R) -> R {
    let mut guard = SESSION.lock().unwrap();
    let session = guard.get_or_insert_with(ImportSession::new);
    f(session)
}

fn sync_import_progress(
    window: &MoggytWindow,
    status: &str,
    counts: &ImportCounts,
    (done, total): (usize, usize),
    locale: Locale,
) {
    window.set_import_status_text(tr(locale, status).into());
    window.set_import_total(total as i32);
    window.set_import_high(counts.high as i32);
    window.set_import_review((counts.review + counts.low) as i32);
    window.set_import_unresolved(counts.unresolved as i32);
    window.set_import_done(done as i32);
}

fn sync_import_modal_visibility(window: &MoggytWindow, stage: ImportStage) {
    if matches!(stage, ImportStage::Matching | ImportStage::Review) {
        window.set_show_import_modal(false);
    }
}

fn sync_import_review(
    window: &MoggytWindow,
    stage: ImportStage,
    review_opt: Option<&ImportReview>,
    counts: &ImportCounts,
    locale: Locale,
) {
    let is_review = matches!(stage, ImportStage::Review);
    window.set_import_review_open(is_review);
    if let Some(rev) = review_opt {
        window.set_import_review_title(rev.playlist_title.as_str().into());
        window.set_import_review_counts(counts_text(counts, locale).into());
        window.set_import_rows(ModelRc::new(VecModel::from(rows_to_slint(rev, locale))));
        window.set_import_selected_row(rev.selected as i32);
        window.set_import_can_save(rev.has_matches());
    } else {
        window.set_import_review_title("".into());
        window.set_import_review_counts("".into());
        window.set_import_rows(ModelRc::new(VecModel::default()));
        window.set_import_selected_row(0);
        window.set_import_can_save(false);
    }
}

fn sync_import_picker(
    window: &MoggytWindow,
    picker_row: Option<usize>,
    review_opt: Option<&ImportReview>,
    locale: Locale,
) {
    window.set_import_picker_open(picker_row.is_some());
    if let Some(row_idx) = picker_row {
        if let Some(row) = review_opt.and_then(|r| r.rows.get(row_idx)) {
            window.set_import_alternatives(ModelRc::new(VecModel::from(alternatives_to_slint(
                row, locale,
            ))));
            window.set_import_alternative_selected(row.chosen_alternative as i32);
            return;
        }
    }
    window.set_import_alternatives(ModelRc::new(VecModel::default()));
    window.set_import_alternative_selected(0);
}

/// Synchronizes the current import session state into the Slint window properties.
pub fn sync_import(window: &MoggytWindow) {
    let (stage, status, counts, (done, total), picker_row, review_opt) = with_session(|s| {
        (s.stage, s.status.clone(), s.counts(), s.progress(), s.picker_row, s.review.clone())
    });
    let locale =
        crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
    window.set_import_busy(matches!(
        stage,
        ImportStage::LoadingSource | ImportStage::Matching | ImportStage::Cancelling
    ));
    window.set_import_running(matches!(stage, ImportStage::Matching | ImportStage::Cancelling));
    window.set_import_cancelling(matches!(stage, ImportStage::Cancelling));
    window.set_import_failed(matches!(stage, ImportStage::Failed));
    sync_import_progress(window, &status, &counts, (done, total), locale);
    sync_import_modal_visibility(window, stage);
    sync_import_review(window, stage, review_opt.as_ref(), &counts, locale);
    sync_import_picker(window, picker_row, review_opt.as_ref(), locale);
}

/// Drains pending background events and synchronizes the window.
pub fn poll_import(window: &MoggytWindow) {
    let outcome = with_session(|s| s.drain());
    if outcome.connection_lost {
        window.global::<ImportUi>().set_connection_lost(true);
    }
    sync_import(window);
}

fn load_source_link(link_text: &str) -> anyhow::Result<SpotifyPlaylist> {
    let store = KeyringSpotifyCredentialStore::moggyt();
    load_spotify_link(link_text, store.load().as_ref())
}

fn load_source_file(file_path: &str) -> anyhow::Result<SpotifyPlaylist> {
    load_playlist_file(std::path::Path::new(file_path))
}

fn on_source_load_success(win: &MoggytWindow, source: SpotifyPlaylist) {
    let cancel = Arc::new(AtomicBool::new(false));
    let rx = spawn_import(
        Arc::new(LiveTrackSearcher::new()),
        source.clone(),
        Arc::clone(&cancel),
        WorkerConfig::default(),
    );
    with_session(|s| {
        s.begin_matching(source, rx);
        s.set_cancel(cancel);
    });
    win.set_show_import_modal(false);
    sync_import(win);
}

fn spawn_source_loader(
    weak: slint::Weak<MoggytWindow>,
    tab: i32,
    link_text: String,
    file_path: String,
    generation: u64,
) {
    std::thread::Builder::new()
        .name("spotify-source-loader".into())
        .spawn(move || {
            let res =
                if tab == 0 { load_source_link(&link_text) } else { load_source_file(&file_path) };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = weak.upgrade() else { return };
                if !with_session(|s| s.accepts_load_result(generation)) {
                    return;
                }
                match res {
                    Ok(source) => on_source_load_success(&win, source),
                    Err(err) => {
                        with_session(|s| s.fail(format!("{err:#}")));
                        sync_import(&win);
                    }
                }
            });
        })
        .expect("spawn spotify source loader thread");
}

fn run_import_file_picker(weak: slint::Weak<MoggytWindow>, title: &str) {
    let Some(path) = RfdFilePicker.pick_import_file(title) else { return };
    let _ = slint::invoke_from_event_loop(move || {
        let Some(win) = weak.upgrade() else { return };
        win.set_import_file_path(path.display().to_string().into());
        win.set_import_tab(1);
    });
}

fn spawn_import_file_picker(weak: slint::Weak<MoggytWindow>, title: String) {
    std::thread::Builder::new()
        .name("import-file-picker".into())
        .spawn(move || run_import_file_picker(weak, &title))
        .expect("spawn import file picker thread");
}

pub(crate) fn reload_local_library(window: &MoggytWindow, store: &dyn PlaylistStore) {
    let local = store.list();
    crate::slint_shell::window::with_ui_state(|st| {
        let catalog: Vec<_> = st
            .library_playlists
            .iter()
            .filter(|p| !crate::domain::is_local_playlist_id(&p.id))
            .cloned()
            .collect();
        st.library_playlists =
            crate::slint_shell::import_state::merge_local_library(&catalog, &local);
        crate::slint_shell::sync::sync_library_and_detail(window, st);
    });
}

fn handle_save_to_library(window: &MoggytWindow) {
    let Some(store) = current_store() else {
        with_session(|s| s.status = "No playlist store installed".into());
        sync_import(window);
        return;
    };
    let locale =
        crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
    match with_session(|s| s.save(store.as_ref())) {
        Ok(saved) => {
            with_session(|s| {
                s.stage = ImportStage::Idle;
                s.status = format!("{}: {}", tr(locale, "Saved playlist"), saved.title);
                s.review = None;
                s.picker_row = None;
            });
            window.set_show_import_modal(false);
            reload_local_library(window, store.as_ref());
            sync_import(window);
        }
        Err(err) => {
            with_session(|s| {
                s.status = format!("{}: {err}", tr(locale, "Failed to save playlist"))
            });
            sync_import(window);
        }
    }
}

fn handle_save_to_queue(window: &MoggytWindow) {
    let (resolved, artwork) = with_session(|s| (s.resolved_tracks(), s.artwork()));
    if resolved.is_empty() {
        let locale =
            crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
        with_session(|s| s.status = tr(locale, "No matched tracks to import").to_string());
        sync_import(window);
        return;
    }
    let tracks: Vec<Track> = resolved.into_iter().map(|t| t.to_track(artwork.clone())).collect();
    crate::slint_shell::window::with_ui_state(|st| {
        let start_idx = enqueue_imported_tracks(st, tracks);
        st.play_track_at_index_in_memory(start_idx);
        crate::slint_shell::window::trigger_playback(window, st);
        crate::slint_shell::sync::sync_window(window, st);
    });
    with_session(|s| s.reset_to_idle());
    window.set_show_import_modal(false);
    sync_import(window);
}

fn handle_save_requested(window: &MoggytWindow) {
    let destination = with_session(|s| s.destination());
    match destination {
        ImportDestination::Library => handle_save_to_library(window),
        ImportDestination::Queue => handle_save_to_queue(window),
    }
}

fn reset_session_to_idle(window: &MoggytWindow) {
    with_session(|s| s.reset_to_idle());
    window.set_show_import_modal(false);
    sync_import(window);
}

fn toggle_session_exclusion(session: &mut ImportSession, row: usize) -> bool {
    session.review.as_mut().map(|r| r.toggle_exclusion(row)).unwrap_or(false)
}

/// Wires import interaction callbacks between MoggytWindow and the import session.
pub fn install_import_callbacks(window: &MoggytWindow) {
    let w = window.as_weak();
    window.on_import_browse_requested(move || {
        let Some(win) = w.upgrade() else { return };
        let locale =
            crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
        let title = tr(locale, "Select a playlist file");
        spawn_import_file_picker(win.as_weak(), title.to_string());
    });

    let w = window.as_weak();
    window.on_import_link_changed(move |text| {
        let Some(win) = w.upgrade() else { return };
        let locale =
            crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
        let (valid, status) = validate_import_link(&text, locale);
        win.set_import_link_valid(valid);
        win.set_import_link_status(status.into());
    });

    let w = window.as_weak();
    window.on_import_start_requested(move || {
        let Some(win) = w.upgrade() else { return };
        if with_session(|s| s.is_busy()) {
            return;
        }
        win.global::<ImportUi>().set_connection_lost(false);
        let tab = win.get_import_tab();
        let link_text = win.get_import_link_text().to_string();
        let file_path = win.get_import_file_path().to_string();
        let name_raw = win.get_import_name_text().to_string();
        let trimmed = name_raw.trim();
        let name_override = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
        let destination = if win.get_import_to_queue() {
            ImportDestination::Queue
        } else {
            ImportDestination::Library
        };
        let generation = with_session(|s| {
            s.set_options(destination, name_override);
            s.start_loading()
        });
        sync_import(&win);
        spawn_source_loader(win.as_weak(), tab, link_text, file_path, generation);
    });

    let w = window.as_weak();
    window.on_import_cancel_requested(move || {
        let Some(win) = w.upgrade() else { return };
        let already_cancelling = with_session(|s| {
            if s.stage == ImportStage::Cancelling {
                return true;
            }
            s.request_cancel();
            false
        });
        if already_cancelling {
            return;
        }
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_close_requested(move || {
        let Some(win) = w.upgrade() else { return };
        reset_session_to_idle(&win);
    });

    let w = window.as_weak();
    window.on_import_discard_requested(move || {
        let Some(win) = w.upgrade() else { return };
        reset_session_to_idle(&win);
    });

    let w = window.as_weak();
    window.on_import_row_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| s.select_row(idx as usize));
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_row_exclusion_toggled(move |idx| {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| toggle_session_exclusion(s, idx as usize));
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_swap_requested(move |idx| {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| s.open_picker(idx as usize));
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_alternative_chosen(move |idx| {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| s.choose_alternative(idx as usize));
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_picker_dismissed(move || {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| s.close_picker());
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_custom_id_submitted(move |text| {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| {
            let Some(row) = s.picker_row else {
                return;
            };
            if s.review.as_mut().is_some_and(|r| r.apply_custom_video_id(row, &text)) {
                s.close_picker();
            }
        });
        sync_import(&win);
    });

    let w = window.as_weak();
    window.on_import_save_requested(move || {
        let Some(win) = w.upgrade() else { return };
        handle_save_requested(&win);
    });

    let w = window.as_weak();
    window.global::<ImportUi>().on_retry_requested(move || {
        let Some(win) = w.upgrade() else { return };
        let Some(source) = with_session(|s| s.source.clone()) else { return };
        win.global::<ImportUi>().set_connection_lost(false);
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn_import(
            Arc::new(LiveTrackSearcher::new()),
            source.clone(),
            Arc::clone(&cancel),
            WorkerConfig::default(),
        );
        with_session(|s| {
            s.begin_matching(source, rx);
            s.set_cancel(cancel);
        });
        sync_import(&win);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::{LocalTrack, MatchConfidence, SpotifyTrack};
    use crate::infra::MemoryPlaylistStore;
    use crate::slint_shell::import_state::delta_text;
    use crate::slint_shell::import_worker::TrackResolution;

    /// Serializes the tests that touch the process-global import session.
    ///
    /// `SESSION` is a single `Mutex<Option<ImportSession>>` shared by the whole
    /// process, so tests that reset or drain it race each other under the
    /// parallel test harness. Taking this lock plus a reset makes each of them
    /// start from a known state.
    static SESSION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_session_tests() -> std::sync::MutexGuard<'static, ()> {
        SESSION_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset_session() {
        with_session(|s| *s = ImportSession::new());
    }

    fn sample_spotify_playlist() -> SpotifyPlaylist {
        SpotifyPlaylist {
            id: "sp_cb_test".to_string(),
            title: "Road Trip".to_string(),
            author: "Tester".to_string(),
            artwork: None,
            tracks: (0..10)
                .map(|i| SpotifyTrack {
                    title: format!("Song {}", i + 1),
                    artist: format!("Artist {}", i + 1),
                    album: None,
                    duration_ms: 180000,
                    isrc: None,
                })
                .collect(),
        }
    }

    #[test]
    fn test_install_and_current_playlist_store() {
        let store: Arc<dyn PlaylistStore> = Arc::new(MemoryPlaylistStore::new());
        install_playlist_store(Arc::clone(&store));
        let retrieved = current_store();
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_with_session_mutates_state() {
        let _serial = lock_session_tests();
        reset_session();
        with_session(|s| {
            assert_eq!(s.start_loading(), 1);
            assert_eq!(s.stage, ImportStage::LoadingSource);
        });
    }

    #[test]
    fn test_load_source_file_success_and_missing() {
        let missing = load_source_file("/path/does/not/exist/never.csv");
        assert!(missing.is_err());
    }

    #[test]
    fn test_reset_session_to_idle() {
        let _serial = lock_session_tests();
        reset_session();
        with_session(|s| {
            s.stage = ImportStage::Failed;
            s.status = "error".into();
            s.picker_row = Some(2);
        });
        with_session(|s| s.reset_to_idle());
        with_session(|s| {
            assert_eq!(s.stage, ImportStage::Idle);
            assert!(s.status.is_empty());
            assert_eq!(s.picker_row, None);
        });
    }

    #[test]
    fn test_load_source_link_invalid_url() {
        assert!(load_source_link("not-a-valid-spotify-url").is_err());
    }

    #[test]
    fn test_poll_import_drains_events_and_updates_status() {
        let _serial = lock_session_tests();
        reset_session();
        let (tx, rx) = std::sync::mpsc::channel();
        with_session(|s| {
            s.begin_matching(sample_spotify_playlist(), rx);
        });
        tx.send(crate::slint_shell::import_worker::ImportProgressEvent::Started { total: 10 })
            .unwrap();
        tx.send(crate::slint_shell::import_worker::ImportProgressEvent::Progress {
            done: 5,
            total: 10,
            high: 5,
            review: 0,
            unresolved: 0,
        })
        .unwrap();

        let outcome = with_session(|s| s.drain());
        assert_eq!(outcome.applied, 2);
        with_session(|s| {
            assert_eq!(s.status, "5 / 10");
        });
    }

    #[test]
    fn test_validate_import_link_table() {
        let (valid, status) = validate_import_link("", Locale::English);
        assert!(!valid);
        assert!(status.is_empty());
        let (valid, status) = validate_import_link("   \t\n  ", Locale::English);
        assert!(!valid);
        assert!(status.is_empty());

        let (valid, status) = validate_import_link("not-a-spotify-link", Locale::English);
        assert!(!valid);
        assert_eq!(status, "Invalid Spotify link");
        let (valid, status) = validate_import_link("not-a-spotify-link", Locale::Spanish);
        assert!(!valid);
        assert_eq!(status, "Enlace de Spotify no valido");

        let (valid, status) = validate_import_link(
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            Locale::English,
        );
        assert!(valid);
        assert_eq!(status, "Valid Spotify link");

        let (valid, status) =
            validate_import_link("spotify:album:4aawyAB9vmqN3uQ7FjRGTy", Locale::English);
        assert!(valid);
        assert_eq!(status, "Valid Spotify link");
    }

    #[test]
    fn test_enqueue_imported_tracks() {
        let mut state = crate::slint_shell::test_support::test_wired();
        assert_eq!(state.queue.len(), 0);

        let t1 = Track::new("t1", "Track 1", "Artist 1");
        let t2 = Track::new("t2", "Track 2", "Artist 2");
        let start = enqueue_imported_tracks(&mut state, vec![t1, t2]);
        assert_eq!(start, 0);
        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.tracks()[0].id, "t1");
        assert_eq!(state.queue.tracks()[1].id, "t2");

        let t3 = Track::new("t3", "Track 3", "Artist 3");
        let start2 = enqueue_imported_tracks(&mut state, vec![t3]);
        assert_eq!(start2, 2);
        assert_eq!(state.queue.len(), 3);
        assert_eq!(state.queue.tracks()[2].id, "t3");
    }

    #[test]
    fn test_delta_text_table() {
        assert_eq!(delta_text(Some(180), Some(183)), "+3s");
        assert_eq!(delta_text(Some(180), Some(178)), "-2s");
        assert_eq!(delta_text(Some(180), Some(180)), "0s");
        assert_eq!(delta_text(None, Some(180)), "");
        assert_eq!(delta_text(Some(180), None), "");
        assert_eq!(delta_text(None, None), "");
    }

    #[test]
    fn test_exclusion_skips_in_selected_tracks_and_to_local_playlist() {
        let playlist = sample_spotify_playlist();
        let t1 = LocalTrack::new("id1", "Song 1", "Artist 1", MatchConfidence::High);
        let t2 = LocalTrack::new("id2", "Song 2", "Artist 2", MatchConfidence::Medium);
        let resolutions = vec![
            TrackResolution::Matched { track: t1, alternatives: Vec::new() },
            TrackResolution::Matched { track: t2, alternatives: Vec::new() },
        ];
        let mut review = ImportReview::from_resolutions(&playlist, &resolutions);
        assert_eq!(review.selected_tracks().len(), 2);

        assert!(review.toggle_exclusion(0));
        let selected = review.selected_tracks();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].video_id, "id2");

        let pl = review.to_local_playlist_with_title(Some("Exclusion Test"));
        assert_eq!(pl.tracks.len(), 1);
        assert_eq!(pl.tracks[0].video_id, "id2");
    }

    #[test]
    fn test_has_matches_false_when_all_chosen_rows_excluded() {
        let playlist = sample_spotify_playlist();
        let t1 = LocalTrack::new("id1", "Song 1", "Artist 1", MatchConfidence::High);
        let resolutions = vec![TrackResolution::Matched { track: t1, alternatives: Vec::new() }];
        let mut review = ImportReview::from_resolutions(&playlist, &resolutions);
        assert!(review.has_matches());

        assert!(review.toggle_exclusion(0));
        assert!(!review.has_matches());
    }

    #[test]
    fn test_channel_and_delta_appear_in_rows_to_slint() {
        let playlist = sample_spotify_playlist();
        let mut t1 = LocalTrack::new("id1", "Track 1", "Channel 1", MatchConfidence::High);
        t1.duration_secs = Some(183);
        let resolutions = vec![
            TrackResolution::Matched { track: t1, alternatives: Vec::new() },
            TrackResolution::Unresolved {
                title: "Song 2".into(),
                artist: "Artist 2".into(),
                reason: "None".into(),
            },
        ];
        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        let slint_rows = rows_to_slint(&review, Locale::English);
        assert_eq!(slint_rows.len(), 2);
        assert_eq!(slint_rows[0].channel, "Channel 1");
        assert_eq!(slint_rows[0].delta, "+3s");
        assert!(!slint_rows[0].excluded);
        assert_eq!(slint_rows[1].channel, "");
        assert_eq!(slint_rows[1].delta, "");
        assert!(!slint_rows[1].excluded);
    }

    #[test]
    fn test_session_toggle_row_exclusion_flips_and_rejects_out_of_range() {
        let mut session = ImportSession::new();
        assert!(!toggle_session_exclusion(&mut session, 0));

        let playlist = sample_spotify_playlist();
        let t1 = LocalTrack::new("id1", "Song 1", "Artist 1", MatchConfidence::High);
        let resolutions = vec![TrackResolution::Matched { track: t1, alternatives: Vec::new() }];
        session.review = Some(ImportReview::from_resolutions(&playlist, &resolutions));

        assert!(toggle_session_exclusion(&mut session, 0));
        assert!(session.review.as_ref().unwrap().rows[0].excluded);
        assert!(toggle_session_exclusion(&mut session, 0));
        assert!(!session.review.as_ref().unwrap().rows[0].excluded);
        assert!(!toggle_session_exclusion(&mut session, 99));
    }

    #[test]
    fn test_custom_video_id_submission_updates_review_and_closes_picker() {
        let mut session = ImportSession::new();
        let playlist = sample_spotify_playlist();
        let t1 = LocalTrack::new("old_id_1111", "Song 1", "Artist 1", MatchConfidence::Low);
        let resolutions = vec![TrackResolution::Matched { track: t1, alternatives: Vec::new() }];
        session.review = Some(ImportReview::from_resolutions(&playlist, &resolutions));
        session.picker_row = Some(0);

        let row = session.picker_row.unwrap();
        let applied =
            session.review.as_mut().is_some_and(|r| r.apply_custom_video_id(row, "dQw4w9WgXcQ"));
        assert!(applied);
        session.close_picker();

        assert_eq!(session.picker_row, None);
        let updated = &session.review.unwrap().rows[0];
        assert_eq!(updated.chosen.as_ref().unwrap().video_id, "dQw4w9WgXcQ");
        assert_eq!(updated.confidence, Some(MatchConfidence::High));
    }
}
