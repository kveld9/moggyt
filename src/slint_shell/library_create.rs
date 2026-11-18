//! Playlist creation from the Library `+` menu.
//!
//! Hexagonal invariant: persistence and network stay behind the Catalog port;
//! this module only wires the dialog to it.

use std::rc::Rc;
use std::sync::mpsc::Sender;

use slint::ComponentHandle;

use crate::domain::DetailKind;
use crate::i18n::{Locale, tr};
use crate::infra::CatalogError;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::toast::notify;
use crate::slint_shell::window::{open_detail_async, refresh_library_async, with_ui_state};
use crate::slint_shell::{ContextIcon, LibraryAddUi, MoggytWindow};

/// Returns the browse identifier formatted for a playlist detail request.
///
/// Returns the id unchanged when it already starts with "VL", otherwise prefixes "VL".
pub(crate) fn playlist_detail_browse_id(playlist_id: &str) -> String {
    let trimmed = playlist_id.trim();
    if trimmed.starts_with("VL") {
        return trimmed.to_owned();
    }
    format!("VL{trimmed}")
}

/// Validates a proposed playlist title and maps errors to localized messages.
pub(crate) fn validate_dialog_title(title: &str, locale: Locale) -> Result<String, &'static str> {
    crate::domain::new_playlist_title(title).map_err(|err| match err {
        crate::domain::PlaylistTitleError::Empty => tr(locale, "Playlist name cannot be empty"),
        crate::domain::PlaylistTitleError::TooLong => tr(locale, "Playlist name is too long"),
        crate::domain::PlaylistTitleError::UnsupportedCharacters => {
            tr(locale, "Playlist name contains unsupported characters")
        }
    })
}

fn apply_creation_outcome(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    outcome: Result<String, CatalogError>,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    let add_ui = win.global::<LibraryAddUi>();
    add_ui.set_create_busy(false);
    match outcome {
        Ok(id) => {
            add_ui.set_create_open(false);
            add_ui.set_playlist_name_text("".into());
            add_ui.set_create_status_text("".into());
            add_ui.set_create_failed(false);
            notify(win, st, tr(st.locale, "Playlist created"), ContextIcon::Open);
            refresh_library_async(win, st, worker_tx);
            let browse_id = playlist_detail_browse_id(&id);
            open_detail_async(win, st, &browse_id, DetailKind::Playlist, worker_tx);
        }
        Err(err) => {
            tracing::warn!("failed to create playlist: {err}");
            add_ui.set_create_failed(true);
            add_ui.set_create_status_text(tr(st.locale, "Could not create the playlist").into());
        }
    }
}

fn submit_creation(
    win: &MoggytWindow,
    state_rc: &SharedState,
    text: &str,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    let add_ui = win.global::<LibraryAddUi>();
    if add_ui.get_create_busy() {
        return;
    }
    let locale = state_rc.borrow().locale;
    let title = match validate_dialog_title(text, locale) {
        Ok(valid_title) => valid_title,
        Err(msg) => {
            add_ui.set_create_failed(true);
            add_ui.set_create_status_text(msg.into());
            return;
        }
    };
    add_ui.set_create_failed(false);
    add_ui.set_create_status_text("".into());
    add_ui.set_create_busy(true);

    let catalog_arc = std::sync::Arc::clone(&state_rc.borrow().catalog);
    let weak_win = win.as_weak();
    let tx = worker_tx.clone();

    let _ = std::thread::Builder::new().name("create-playlist".to_owned()).spawn(move || {
        let outcome = catalog_arc
            .lock()
            .map_err(|_| CatalogError::Transport)
            .and_then(|cat| cat.create_playlist(&title));
        let _ = dispatch_to_ui(move || {
            let Some(w) = weak_win.upgrade() else { return };
            with_ui_state(|st| {
                apply_creation_outcome(&w, st, outcome, &tx);
            });
        });
    });
}

/// Wires the library add global callbacks to MoggytWindow and application state.
pub(crate) fn wire(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    let w = window.as_weak();
    window.global::<LibraryAddUi>().on_menu_dismissed(move || {
        let Some(win) = w.upgrade() else { return };
        win.global::<LibraryAddUi>().set_menu_open(false);
    });

    let w = window.as_weak();
    window.global::<LibraryAddUi>().on_new_playlist_requested(move || {
        let Some(win) = w.upgrade() else { return };
        let add_ui = win.global::<LibraryAddUi>();
        add_ui.set_menu_open(false);
        add_ui.set_create_open(true);
        add_ui.set_playlist_name_text("".into());
        add_ui.set_create_status_text("".into());
        add_ui.set_create_failed(false);
        add_ui.set_create_busy(false);
    });

    let w = window.as_weak();
    window.global::<LibraryAddUi>().on_import_requested(move || {
        let Some(win) = w.upgrade() else { return };
        win.global::<LibraryAddUi>().set_menu_open(false);
        win.set_show_import_modal(true);
    });

    let w = window.as_weak();
    window.global::<LibraryAddUi>().on_create_dismissed(move || {
        let Some(win) = w.upgrade() else { return };
        let add_ui = win.global::<LibraryAddUi>();
        add_ui.set_create_open(false);
        add_ui.set_create_busy(false);
        add_ui.set_create_status_text("".into());
        add_ui.set_create_failed(false);
        add_ui.set_playlist_name_text("".into());
    });

    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    let tx = worker_tx.clone();
    window.global::<LibraryAddUi>().on_submit_requested(move |text| {
        let Some(win) = w.upgrade() else { return };
        submit_creation(&win, &s, text.as_str(), &tx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_detail_browse_id() {
        assert_eq!(playlist_detail_browse_id("PL1"), "VLPL1");
        assert_eq!(playlist_detail_browse_id("VLPL1"), "VLPL1");
        assert_eq!(playlist_detail_browse_id("  VLabc  "), "VLabc");
        assert_eq!(playlist_detail_browse_id("  PLabc  "), "VLPLabc");
    }

    #[test]
    fn test_validate_dialog_title() {
        assert_eq!(
            validate_dialog_title("  My Playlist  ", Locale::English),
            Ok("My Playlist".to_owned())
        );
        assert_eq!(
            validate_dialog_title("", Locale::English),
            Err("Playlist name cannot be empty")
        );
        assert_eq!(
            validate_dialog_title("   ", Locale::English),
            Err("Playlist name cannot be empty")
        );
        assert_eq!(
            validate_dialog_title("a<b", Locale::English),
            Err("Playlist name contains unsupported characters")
        );
        assert_eq!(
            validate_dialog_title("a>b", Locale::English),
            Err("Playlist name contains unsupported characters")
        );

        let long_title = "a".repeat(151);
        assert_eq!(
            validate_dialog_title(&long_title, Locale::English),
            Err("Playlist name is too long")
        );

        // Spanish localized message verification
        assert_eq!(
            validate_dialog_title("", Locale::Spanish),
            Err("El nombre de la playlist no puede estar vac\u{00ed}o")
        );
    }
}
