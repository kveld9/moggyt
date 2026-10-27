//! Library page view.

use eframe::egui;

use crate::app::components::{empty_state, section_header, track_row};
use crate::app::{AppState, UpdateStatus, theme};
use crate::domain::{DesignTokens, ThemeId, Track};
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
    let tokens = DesignTokens::desktop();
    render_library_preferences(ui, state);
    render_library_separator(ui, &tokens);
    render_library_lastfm(ui, state);
    render_library_separator(ui, &tokens);
    render_library_sign_in(ui, state);
    render_library_separator(ui, &tokens);
    render_library_updates(ui, state);
    render_library_separator(ui, &tokens);
    render_library_backup(ui, state);
    render_library_separator(ui, &tokens);
    render_library_import(ui, state);
    render_library_separator(ui, &tokens);
    render_library_playlists(ui, state);
    render_library_separator(ui, &tokens);
    render_library_liked(ui, state);
    render_library_separator(ui, &tokens);
    render_library_albums(ui, state);
}

fn render_library_separator(ui: &mut egui::Ui, tokens: &DesignTokens) {
    ui.add_space(tokens.space_sm);
    ui.separator();
    ui.add_space(tokens.space_xs);
}

fn render_status_caption(ui: &mut egui::Ui, text: &str) {
    let tokens = DesignTokens::desktop();
    ui.label(
        egui::RichText::new(text).size(tokens.font_caption).color(ui.visuals().weak_text_color()),
    );
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
    let tokens = DesignTokens::desktop();
    section_header(ui, tr(state.locale, "Preferences"));
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}:", tr(state.locale, "Theme"))).size(tokens.font_body),
        );
        render_theme_button(ui, state, ThemeId::White, "White");
        render_theme_button(ui, state, ThemeId::BlackOled, "Black OLED");
        render_theme_button(ui, state, ThemeId::Mono, "Mono");
    });
    ui.add_space(tokens.space_xs);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}:", tr(state.locale, "Language")))
                .size(tokens.font_body),
        );
        render_locale_button(ui, state, Locale::English, "English");
        render_locale_button(ui, state, Locale::Spanish, "Spanish");
    });
    ui.add_space(tokens.space_xs);
    let mut shortcuts = state.shortcuts_enabled;
    if ui.checkbox(&mut shortcuts, "Keyboard shortcuts").changed() {
        state.set_shortcuts_enabled(shortcuts);
    }
    ui.add_space(tokens.space_xxs);
    let mut hi_res = state.hi_res_covers;
    if ui.checkbox(&mut hi_res, "High-resolution covers").changed() {
        state.set_hi_res_covers(hi_res);
    }
    render_dsp_preferences(ui, state);
}

/// Renders DSP audio playback preferences (skip silence, loudness normalization).
pub(crate) fn render_dsp_preferences<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let mut skip_silence = state.skip_silence;
    if ui.checkbox(&mut skip_silence, "Skip silence").changed() {
        state.set_skip_silence(skip_silence);
    }
    let mut loudness = state.loudness_normalization;
    if ui.checkbox(&mut loudness, "Loudness normalization").changed() {
        state.set_loudness_normalization(loudness);
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
    let tokens = DesignTokens::desktop();
    section_header(ui, "Last.fm");
    let mut enabled = state.lastfm_enabled;
    if ui.checkbox(&mut enabled, "Enable scrobbling").changed() {
        state.set_lastfm_enabled(enabled);
    }
    ui.add_space(tokens.space_xxs);
    let status = if state.lastfm_is_live() { "Scrobbler: Live" } else { "Scrobbler: Dormant" };
    render_status_caption(ui, status);
    ui.add_space(tokens.space_xs);
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
    let tokens = DesignTokens::desktop();
    render_lastfm_field(ui, &mut state.lastfm_api_key_text, &mut state.text_focused, "API Key");
    render_lastfm_field(ui, &mut state.lastfm_secret_text, &mut state.text_focused, "Secret");
    render_lastfm_field(
        ui,
        &mut state.lastfm_session_key_text,
        &mut state.text_focused,
        "Session Key",
    );
    ui.add_space(tokens.space_xs);
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
    let tokens = DesignTokens::desktop();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{label}:")).size(tokens.font_body));
        let response = ui.add(egui::TextEdit::singleline(text).password(true));
        if response.has_focus() {
            *focused = true;
        }
    });
    ui.add_space(tokens.space_xxs);
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
    let tokens = DesignTokens::desktop();
    let label = tr(state.locale, key);
    let text = egui::RichText::new(label).size(tokens.font_body);
    let is_selected = state.theme == theme;
    if ui.selectable_label(is_selected, text).clicked() && !is_selected {
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
    let tokens = DesignTokens::desktop();
    let label = tr(state.locale, key);
    let text = egui::RichText::new(label).size(tokens.font_body);
    let is_selected = state.locale == locale;
    if ui.selectable_label(is_selected, text).clicked() && !is_selected {
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
    let tokens = DesignTokens::desktop();
    section_header(ui, "Updates");
    if render_update_status_body(ui, &state.update_status) {
        state.dismiss_update();
    }
    ui.add_space(tokens.space_xs);
    if ui.button("Check for updates").clicked() {
        state.refresh_update();
    }
}

/// Renders updater status message or available update banner.
pub(crate) fn render_update_status_body(ui: &mut egui::Ui, status: &UpdateStatus) -> bool {
    match status {
        UpdateStatus::Unknown => {
            render_status_caption(ui, "Never checked");
        }
        UpdateStatus::Current => {
            let message = format!("moggyt is up to date (v{})", GithubUpdater::current_version());
            render_status_caption(ui, &message);
        }
        UpdateStatus::Available { version, url } => {
            return render_update_available(ui, version, url);
        }
        UpdateStatus::CheckFailed => {
            render_status_caption(ui, "Update check failed (offline?)");
        }
    }
    false
}

/// Renders release download link and dismiss control for available updates.
pub(crate) fn render_update_available(ui: &mut egui::Ui, version: &str, url: &str) -> bool {
    let tokens = DesignTokens::desktop();
    ui.label(
        egui::RichText::new(format!("Update available: {version}")).size(tokens.font_body).strong(),
    );
    ui.add_space(tokens.space_xxs);
    let mut dismissed = false;
    ui.horizontal(|ui| {
        ui.hyperlink_to("Download page", url);
        if ui.button("Dismiss").clicked() {
            dismissed = true;
        }
    });
    dismissed
}

/// Renders library backup export and import controls.
pub(crate) fn render_library_backup<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Library Backup");
    if ui.button("Export").clicked() {
        let _ = state.export_library_backup();
    }
    render_backup_import_row(ui, state);
    if !state.backup_status_text.is_empty() {
        ui.label(&state.backup_status_text);
    }
}

/// Renders the single-line backup path input row and import trigger.
pub(crate) fn render_backup_import_row<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.horizontal(|ui| {
        ui.label("Import path:");
        let response = ui.text_edit_singleline(&mut state.backup_path_text);
        if response.has_focus() {
            state.text_focused = true;
        }
        if ui.button("Import").clicked() {
            let path = state.backup_path_text.clone();
            let _ = state.import_library_backup(&path);
        }
    });
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
    let tokens = DesignTokens::desktop();
    section_header(ui, "Sign in");
    let status = if state.is_signed_in() {
        "Signed in — library unlocked"
    } else {
        "Anonymous — paste cookies to unlock library"
    };
    render_status_caption(ui, status);
    ui.add_space(tokens.space_xs);
    render_cookie_input_row(ui, state);
    ui.add_space(tokens.space_xs);
    render_companion_handoff_row(ui, state);
}

fn render_cookie_input_row<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
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

fn render_companion_handoff_row<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.horizontal(|ui| {
        ui.label("Companion import:");
        let response = ui.text_edit_singleline(&mut state.handoff_text);
        if response.has_focus() {
            state.text_focused = true;
        }
        if ui.button("Import").clicked() {
            state.import_handoff();
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
    section_header(ui, "Playlists");
    let playlists = state.catalog.playlists();
    if playlists.is_empty() {
        empty_state(ui, "No playlists");
        return;
    }
    for playlist in &playlists {
        let subtitle = format!("({} tracks)", playlist.track_count);
        track_row(ui, &playlist.title, &subtitle, false);
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
    section_header(ui, "Liked songs");
    let liked = state.catalog.liked_tracks();
    if liked.is_empty() {
        empty_state(ui, "No liked tracks");
        return;
    }
    let current_id = state.queue.current().map(|t| t.id.as_str());
    if let Some(id) = render_liked_tracks(ui, &liked, current_id) {
        state.toggle_like(&id);
    }
}

fn render_liked_tracks(
    ui: &mut egui::Ui,
    liked: &[Track],
    current_id: Option<&str>,
) -> Option<String> {
    let mut to_toggle = None;
    for track in liked {
        if render_liked_item(ui, track, current_id) {
            to_toggle = Some(track.id.clone());
        }
    }
    to_toggle
}

fn render_liked_item(ui: &mut egui::Ui, track: &Track, current_track_id: Option<&str>) -> bool {
    let mut unliked = false;
    ui.horizontal(|ui| {
        let is_active = current_track_id == Some(track.id.as_str());
        track_row(ui, &track.title, &track.artist, is_active);
        if ui.button("Unlike").clicked() {
            unliked = true;
        }
    });
    unliked
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
    section_header(ui, "Saved albums");
    let albums = state.catalog.saved_albums();
    if albums.is_empty() {
        empty_state(ui, "No saved albums");
        return;
    }
    for album in &albums {
        track_row(ui, &album.title, &album.artist, false);
    }
}

/// Renders playlist URL import controls and status line.
pub(crate) fn render_library_import<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Import playlist");
    render_import_input_row(ui, state);
    render_import_status(ui, state.playlist_import_status.as_deref());
}

fn render_import_input_row<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.horizontal(|ui| {
        let response = ui.text_edit_singleline(&mut state.playlist_url_text);
        if response.has_focus() {
            state.text_focused = true;
        }
        if ui.button("Import").clicked() {
            handle_import_click(state);
        }
    });
}

pub(crate) fn handle_import_click<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    state: &mut AppState<C, A, S, L, K>,
) {
    let url = state.playlist_url_text.clone();
    match state.import_playlist_url(&url) {
        Ok(count) => {
            state.playlist_import_status = Some(format!("Imported {count} track(s) to queue"));
            state.playlist_url_text.clear();
        }
        Err(err) => {
            state.playlist_import_status = Some(format!("Import error: {err}"));
        }
    }
}

fn render_import_status(ui: &mut egui::Ui, status: Option<&str>) {
    if let Some(msg) = status {
        ui.label(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore};

    fn test_state()
    -> AppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache> {
        AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        )
    }

    #[test]
    fn handle_import_click_success_updates_status_and_clears_input() {
        let mut state = test_state();
        state.playlist_url_text = "https://music.youtube.com/playlist?list=PLtest_123".into();
        handle_import_click(&mut state);
        assert_eq!(state.playlist_import_status.as_deref(), Some("Imported 2 track(s) to queue"));
        assert!(state.playlist_url_text.is_empty());
        assert_eq!(state.queue.len(), 2);
    }

    #[test]
    fn handle_import_click_failure_sets_error_status_and_preserves_input() {
        let mut state = test_state();
        let invalid = "https://example.com/playlist?list=PLtest_123";
        state.playlist_url_text = invalid.into();
        handle_import_click(&mut state);
        assert!(state.playlist_import_status.as_ref().unwrap().starts_with("Import error:"));
        assert_eq!(state.playlist_url_text, invalid);
        assert!(state.queue.is_empty());
    }
}
