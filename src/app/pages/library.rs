//! Library page view.

use eframe::egui;

use crate::app::{AppState, UpdateStatus, theme};
use crate::domain::ThemeId;
use crate::i18n::{Locale, tr};
use crate::infra::{AudioBackend, Catalog, CoverCache, GithubUpdater, LyricsSource, SessionStore};

/// Renders the library page view including preferences, Last.fm, sign-in, updates, and collections.
pub(crate) fn render_library<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_library_preferences(ui, state);
    ui.separator();
    render_library_lastfm(ui, state);
    ui.separator();
    render_library_sign_in(ui, state);
    ui.separator();
    render_library_updates(ui, state);
    ui.separator();
    render_library_playlists(ui, state);
    ui.separator();
    render_library_liked(ui, state);
    ui.separator();
    render_library_albums(ui, state);
}

/// Renders theme, locale, and behavioral preferences.
pub(crate) fn render_library_preferences<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading(tr(state.locale, "Theme"));
    ui.horizontal(|ui| {
        render_theme_button(ui, state, ThemeId::White, "White");
        render_theme_button(ui, state, ThemeId::BlackOled, "Black OLED");
        render_theme_button(ui, state, ThemeId::Mono, "Mono");
    });
    ui.add_space(4.0);
    ui.heading(tr(state.locale, "Language"));
    ui.horizontal(|ui| {
        render_locale_button(ui, state, Locale::English, "English");
        render_locale_button(ui, state, Locale::Spanish, "Spanish");
    });
    ui.add_space(4.0);
    let mut shortcuts = state.shortcuts_enabled;
    if ui.checkbox(&mut shortcuts, "Keyboard shortcuts").changed() {
        state.set_shortcuts_enabled(shortcuts);
    }
    let mut hi_res = state.hi_res_covers;
    if ui.checkbox(&mut hi_res, "High-resolution covers").changed() {
        state.set_hi_res_covers(hi_res);
    }
}

/// Renders Last.fm configuration and status.
pub(crate) fn render_library_lastfm<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Last.fm");
    let mut enabled = state.lastfm_enabled;
    if ui.checkbox(&mut enabled, "Enable scrobbling").changed() {
        state.set_lastfm_enabled(enabled);
    }
    let status = if state.lastfm_is_live() { "Scrobbler: Live" } else { "Scrobbler: Dormant" };
    ui.label(status);
    render_lastfm_inputs(ui, state);
}

/// Renders Last.fm credential input fields and persistence buttons.
pub(crate) fn render_lastfm_inputs<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_lastfm_field(ui, &mut state.lastfm_api_key_text, &mut state.text_focused, "API Key");
    render_lastfm_field(ui, &mut state.lastfm_secret_text, &mut state.text_focused, "Secret");
    render_lastfm_field(
        ui,
        &mut state.lastfm_session_key_text,
        &mut state.text_focused,
        "Session Key",
    );
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            state.save_lastfm_credentials_from_input();
        }
        if ui.button("Clear").clicked() {
            state.clear_lastfm_credentials();
        }
    });
}

/// Renders a single masked credential text input field.
pub(crate) fn render_lastfm_field(
    ui: &mut egui::Ui,
    text: &mut String,
    focused: &mut bool,
    label: &str,
) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        let response = ui.add(egui::TextEdit::singleline(text).password(true));
        if response.has_focus() {
            *focused = true;
        }
    });
}

/// Renders a selectable theme button and applies selection changes.
pub(crate) fn render_theme_button<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    theme: ThemeId,
    key: &str,
) {
    let label = tr(state.locale, key);
    let is_selected = state.theme == theme;
    if ui.selectable_label(is_selected, label).clicked() && !is_selected {
        state.set_theme(theme);
        theme::apply(ui.ctx(), &theme.palette());
    }
}

/// Renders a selectable locale button and updates active locale.
pub(crate) fn render_locale_button<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    locale: Locale,
    key: &str,
) {
    let label = tr(state.locale, key);
    let is_selected = state.locale == locale;
    if ui.selectable_label(is_selected, label).clicked() && !is_selected {
        state.set_locale(locale);
    }
}

/// Renders updater check status and manual check trigger.
pub(crate) fn render_library_updates<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Updates");
    if render_update_status_body(ui, &state.update_status) {
        state.dismiss_update();
    }
    if ui.button("Check for updates").clicked() {
        state.refresh_update();
    }
}

/// Renders updater status message or available update banner.
pub(crate) fn render_update_status_body(ui: &mut egui::Ui, status: &UpdateStatus) -> bool {
    match status {
        UpdateStatus::Unknown => {
            ui.label("Never checked");
        }
        UpdateStatus::Current => {
            ui.label(format!("moggyt is up to date (v{})", GithubUpdater::current_version()));
        }
        UpdateStatus::Available { version, url } => {
            return render_update_available(ui, version, url);
        }
        UpdateStatus::CheckFailed => {
            ui.label("Update check failed (offline?)");
        }
    }
    false
}

/// Renders release download link and dismiss control for available updates.
pub(crate) fn render_update_available(ui: &mut egui::Ui, version: &str, url: &str) -> bool {
    ui.label(format!("Update available: {version}"));
    let mut dismissed = false;
    ui.horizontal(|ui| {
        ui.hyperlink_to("Download page", url);
        if ui.button("Dismiss").clicked() {
            dismissed = true;
        }
    });
    dismissed
}

/// Renders session status and cookie input controls.
pub(crate) fn render_library_sign_in<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let status = if state.is_signed_in() {
        "Signed in — library unlocked"
    } else {
        "Anonymous — paste cookies to unlock library"
    };
    ui.label(status);
    ui.horizontal(|ui| {
        let response = ui.text_edit_singleline(&mut state.cookie_text);
        state.text_focused = response.has_focus();
        if ui.button("Save").clicked() {
            state.sign_in_from_input();
        }
        if ui.button("Clear").clicked() {
            state.sign_out();
        }
    });
}

/// Renders user playlists collection summary.
pub(crate) fn render_library_playlists<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    ui.heading("Playlists");
    let playlists = state.catalog.playlists();
    if playlists.is_empty() {
        ui.label("No playlists");
        return;
    }
    for playlist in playlists {
        ui.label(format!("{} ({} tracks)", playlist.title, playlist.track_count));
    }
}

/// Renders liked tracks list with unlike action buttons.
pub(crate) fn render_library_liked<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Liked songs");
    let liked = state.catalog.liked_tracks();
    if liked.is_empty() {
        ui.label("No liked tracks");
        return;
    }
    let mut to_toggle = None;
    for track in &liked {
        ui.horizontal(|ui| {
            ui.label(track.display());
            if ui.button("Unlike").clicked() {
                to_toggle = Some(track.id.clone());
            }
        });
    }
    if let Some(id) = to_toggle {
        state.toggle_like(&id);
    }
}

/// Renders saved albums collection summary.
pub(crate) fn render_library_albums<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    ui.heading("Saved albums");
    let albums = state.catalog.saved_albums();
    if albums.is_empty() {
        ui.label("No saved albums");
        return;
    }
    for album in albums {
        ui.label(format!("{} - {}", album.title, album.artist));
    }
}
