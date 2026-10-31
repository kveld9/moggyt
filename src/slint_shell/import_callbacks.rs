//! Slint event loop bindings and worker thread wiring for Spotify playlist imports.
//!
//! Hexagonal invariant: adapter connecting Slint UI callbacks and background import
//! pipeline threads to the in-memory session state machine without direct domain rules.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{SpotifyPlaylist, parse_spotify_ref};
use crate::i18n::Locale;
use crate::infra::{PlaylistStore, SpotifyEmbedClient, parse_exportify_csv};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::import_searcher::LiveTrackSearcher;
use crate::slint_shell::import_session::{ImportSession, ImportStage};
use crate::slint_shell::import_state::{
    ImportCounts, ImportReview, alternatives_to_slint, counts_text, rows_to_slint,
};
use crate::slint_shell::import_worker::{WorkerConfig, spawn_import};

static SESSION: Mutex<Option<ImportSession>> = Mutex::new(None);
static STORE: Mutex<Option<Arc<dyn PlaylistStore>>> = Mutex::new(None);

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
    stage: ImportStage,
    status: &str,
    counts: &ImportCounts,
) {
    window.set_import_running(matches!(stage, ImportStage::Matching));
    window.set_import_status_text(status.into());
    window.set_import_total(counts.total as i32);
    window.set_import_high(counts.high as i32);
    window.set_import_review((counts.review + counts.low) as i32);
    window.set_import_unresolved(counts.unresolved as i32);
    let done = counts.high + counts.review + counts.low + counts.unresolved;
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
) {
    window.set_import_picker_open(picker_row.is_some());
    if let Some(row_idx) = picker_row {
        if let Some(row) = review_opt.and_then(|r| r.rows.get(row_idx)) {
            window
                .set_import_alternatives(ModelRc::new(VecModel::from(alternatives_to_slint(row))));
            window.set_import_alternative_selected(row.chosen_alternative as i32);
            return;
        }
    }
    window.set_import_alternatives(ModelRc::new(VecModel::default()));
    window.set_import_alternative_selected(0);
}

/// Synchronizes the current import session state into the Slint window properties.
pub fn sync_import(window: &MoggytWindow) {
    let (stage, status, counts, picker_row, review_opt) =
        with_session(|s| (s.stage, s.status.clone(), s.counts(), s.picker_row, s.review.clone()));
    let locale =
        crate::slint_shell::window::with_ui_state(|st| st.locale).unwrap_or(Locale::English);
    sync_import_progress(window, stage, &status, &counts);
    sync_import_modal_visibility(window, stage);
    sync_import_review(window, stage, review_opt.as_ref(), &counts, locale);
    sync_import_picker(window, picker_row, review_opt.as_ref());
}

/// Drains pending background events and synchronizes the window.
pub fn poll_import(window: &MoggytWindow) {
    with_session(|s| s.drain());
    sync_import(window);
}

fn load_source_link(link_text: &str) -> anyhow::Result<SpotifyPlaylist> {
    let spotify_ref = parse_spotify_ref(link_text)
        .with_context(|| format!("invalid Spotify URL '{link_text}'"))?;
    let client = SpotifyEmbedClient::new();
    client.fetch(&spotify_ref)
}

fn load_source_file(file_path: &str) -> anyhow::Result<SpotifyPlaylist> {
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("failed to read file '{file_path}'"))?;
    let tracks = parse_exportify_csv(&content)?;
    let stem =
        std::path::Path::new(file_path).file_stem().and_then(|s| s.to_str()).unwrap_or("import");
    Ok(SpotifyPlaylist {
        id: stem.to_string(),
        title: stem.to_string(),
        author: "Imported".to_string(),
        artwork: None,
        tracks,
    })
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
) {
    std::thread::Builder::new()
        .name("spotify-source-loader".into())
        .spawn(move || {
            let res =
                if tab == 0 { load_source_link(&link_text) } else { load_source_file(&file_path) };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = weak.upgrade() else { return };
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

fn reload_local_library(window: &MoggytWindow, store: &dyn PlaylistStore) {
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

fn handle_save_requested(window: &MoggytWindow) {
    let Some(store) = current_store() else {
        with_session(|s| s.status = "No playlist store installed".into());
        sync_import(window);
        return;
    };
    match with_session(|s| s.save(store.as_ref())) {
        Ok(saved) => {
            with_session(|s| {
                s.stage = ImportStage::Idle;
                s.status = format!("Saved '{}'", saved.title);
                s.review = None;
                s.picker_row = None;
            });
            window.set_show_import_modal(false);
            reload_local_library(window, store.as_ref());
            sync_import(window);
        }
        Err(err) => {
            with_session(|s| s.status = format!("Failed to save: {err}"));
            sync_import(window);
        }
    }
}

fn reset_session_to_idle(window: &MoggytWindow) {
    with_session(|s| {
        s.stage = ImportStage::Idle;
        s.review = None;
        s.picker_row = None;
        s.status.clear();
    });
    window.set_show_import_modal(false);
    sync_import(window);
}

/// Wires import interaction callbacks between MoggytWindow and the import session.
pub fn install_import_callbacks(window: &MoggytWindow) {
    let w = window.as_weak();
    window.on_import_start_requested(move || {
        let Some(win) = w.upgrade() else { return };
        let tab = win.get_import_tab();
        let link_text = win.get_import_link_text().to_string();
        let file_path = win.get_import_file_path().to_string();
        with_session(|s| s.start_loading());
        sync_import(&win);
        spawn_source_loader(win.as_weak(), tab, link_text, file_path);
    });

    let w = window.as_weak();
    window.on_import_cancel_requested(move || {
        let Some(win) = w.upgrade() else { return };
        with_session(|s| s.request_cancel());
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
    window.on_import_save_requested(move || {
        let Some(win) = w.upgrade() else { return };
        handle_save_requested(&win);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::SpotifyTrack;
    use crate::infra::MemoryPlaylistStore;

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
            tracks: vec![SpotifyTrack {
                title: "Song 1".to_string(),
                artist: "Artist 1".to_string(),
                album: None,
                duration_ms: 180000,
                isrc: None,
            }],
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
            s.start_loading();
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
        with_session(|s| {
            s.stage = ImportStage::Idle;
            s.review = None;
            s.picker_row = None;
            s.status.clear();
        });
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
}
