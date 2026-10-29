//! Localized static user interface labels and window property bindings.
//!
//! Hexagonal invariant: pure in-memory label resolution and Slint window property dispatch.

use crate::i18n::{Locale, tr};
use crate::slint_shell::MoggytWindow;

/// Localized static user interface labels for navigation, views, and settings chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticLabels {
    pub nav_search: &'static str,
    pub nav_explore: &'static str,
    pub nav_library: &'static str,
    pub nav_now_playing: &'static str,
    pub search_placeholder: &'static str,
    pub search_button: &'static str,
    pub filter_all: &'static str,
    pub filter_songs: &'static str,
    pub filter_albums: &'static str,
    pub filter_artists: &'static str,
    pub filter_playlists: &'static str,
    pub search_empty: &'static str,
    pub queue_empty: &'static str,
    pub no_cover: &'static str,
    pub lyrics_unavailable: &'static str,
    pub lyrics_hint: &'static str,
    pub session_anonymous_hint: &'static str,
    pub session_cookie_placeholder: &'static str,
    pub sign_in: &'static str,
    pub session_signed_in_hint: &'static str,
    pub sign_out: &'static str,
    pub section_playlists: &'static str,
    pub empty_playlists: &'static str,
    pub section_albums: &'static str,
    pub empty_albums: &'static str,
    pub section_artists: &'static str,
    pub empty_artists: &'static str,
    pub empty_detail: &'static str,
    pub play_button: &'static str,
    pub shuffle_button: &'static str,
    pub empty_songs: &'static str,
    pub section_theme: &'static str,
    pub theme_white: &'static str,
    pub theme_black_oled: &'static str,
    pub theme_mono: &'static str,
    pub section_playback: &'static str,
    pub playback_speed: &'static str,
    pub section_session: &'static str,
    pub status_signed_in: &'static str,
    pub status_anonymous: &'static str,
    pub section_about: &'static str,
    pub app_version: &'static str,
    pub update_status: &'static str,
    pub explore_loading: &'static str,
    pub explore_empty: &'static str,
}

impl StaticLabels {
    /// Resolves all static labels for the given locale.
    pub fn resolve(locale: Locale) -> Self {
        Self {
            nav_search: tr(locale, "Search"),
            nav_explore: tr(locale, "Explore"),
            nav_library: tr(locale, "Library"),
            nav_now_playing: tr(locale, "Now Playing"),
            search_placeholder: tr(locale, "Search songs..."),
            search_button: tr(locale, "Search"),
            filter_all: tr(locale, "All"),
            filter_songs: tr(locale, "Songs"),
            filter_albums: tr(locale, "Albums"),
            filter_artists: tr(locale, "Artists"),
            filter_playlists: tr(locale, "Playlists"),
            search_empty: tr(locale, "No results found."),
            queue_empty: tr(locale, "The queue is empty."),
            no_cover: tr(locale, "No cover"),
            lyrics_unavailable: tr(locale, "Lyrics unavailable"),
            lyrics_hint: tr(locale, "Synchronized lyrics will appear during playback"),
            session_anonymous_hint: tr(
                locale,
                "Anonymous session: paste cookies to unlock library",
            ),
            session_cookie_placeholder: tr(locale, "Paste session cookies..."),
            sign_in: tr(locale, "Sign in"),
            session_signed_in_hint: tr(locale, "Signed in: library unlocked"),
            sign_out: tr(locale, "Sign out"),
            section_playlists: tr(locale, "Playlists"),
            empty_playlists: tr(locale, "No playlists found."),
            section_albums: tr(locale, "Albums"),
            empty_albums: tr(locale, "No albums found."),
            section_artists: tr(locale, "Artists"),
            empty_artists: tr(locale, "No artists found."),
            empty_detail: tr(locale, "No details available."),
            play_button: tr(locale, "Play"),
            shuffle_button: tr(locale, "Shuffle"),
            empty_songs: tr(locale, "No songs available."),
            section_theme: tr(locale, "Theme"),
            theme_white: tr(locale, "White"),
            theme_black_oled: tr(locale, "Black OLED"),
            theme_mono: tr(locale, "Mono"),
            section_playback: tr(locale, "Playback"),
            playback_speed: tr(locale, "Playback speed"),
            section_session: tr(locale, "Session"),
            status_signed_in: tr(locale, "Status: Signed in"),
            status_anonymous: tr(locale, "Status: Anonymous"),
            section_about: tr(locale, "About"),
            app_version: tr(locale, "App version"),
            update_status: tr(locale, "Update status"),
            explore_loading: tr(locale, "Loading explore..."),
            explore_empty: tr(locale, "No sections found in Explore."),
        }
    }

    /// Pushes all resolved static labels into Slint window properties.
    pub fn apply(&self, window: &MoggytWindow) {
        window.set_nav_search_label(self.nav_search.into());
        window.set_nav_explore_label(self.nav_explore.into());
        window.set_nav_library_label(self.nav_library.into());
        window.set_nav_now_playing_label(self.nav_now_playing.into());

        window.set_search_placeholder(self.search_placeholder.into());
        window.set_search_button_label(self.search_button.into());
        window.set_filter_all_label(self.filter_all.into());
        window.set_filter_songs_label(self.filter_songs.into());
        window.set_filter_albums_label(self.filter_albums.into());
        window.set_filter_artists_label(self.filter_artists.into());
        window.set_filter_playlists_label(self.filter_playlists.into());
        window.set_search_empty_text(self.search_empty.into());

        window.set_queue_empty_text(self.queue_empty.into());

        window.set_no_cover_text(self.no_cover.into());
        window.set_lyrics_unavailable_text(self.lyrics_unavailable.into());
        window.set_lyrics_hint_text(self.lyrics_hint.into());

        window.set_session_anonymous_hint(self.session_anonymous_hint.into());
        window.set_session_cookie_placeholder(self.session_cookie_placeholder.into());
        window.set_sign_in_label(self.sign_in.into());
        window.set_session_signed_in_hint(self.session_signed_in_hint.into());
        window.set_sign_out_label(self.sign_out.into());
        window.set_section_playlists_label(self.section_playlists.into());
        window.set_empty_playlists_text(self.empty_playlists.into());
        window.set_section_albums_label(self.section_albums.into());
        window.set_empty_albums_text(self.empty_albums.into());
        window.set_section_artists_label(self.section_artists.into());
        window.set_empty_artists_text(self.empty_artists.into());

        window.set_empty_detail_text(self.empty_detail.into());
        window.set_play_button_label(self.play_button.into());
        window.set_shuffle_button_label(self.shuffle_button.into());
        window.set_empty_songs_text(self.empty_songs.into());

        window.set_section_theme_label(self.section_theme.into());
        window.set_theme_white_label(self.theme_white.into());
        window.set_theme_black_oled_label(self.theme_black_oled.into());
        window.set_theme_mono_label(self.theme_mono.into());
        window.set_section_playback_label(self.section_playback.into());
        window.set_playback_speed_label(self.playback_speed.into());
        window.set_section_session_label(self.section_session.into());
        window.set_status_signed_in_label(self.status_signed_in.into());
        window.set_status_anonymous_label(self.status_anonymous.into());
        window.set_section_about_label(self.section_about.into());
        window.set_app_version_label(self.app_version.into());
        window.set_update_status_label(self.update_status.into());
        window.set_explore_loading_text(self.explore_loading.into());
        window.set_explore_empty_text(self.explore_empty.into());
    }
}

/// Synchronizes static user interface labels into Slint window properties based on locale.
pub fn sync_static_labels(window: &MoggytWindow, locale: Locale) {
    StaticLabels::resolve(locale).apply(window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_labels_resolution_and_sync() {
        let en_labels = StaticLabels::resolve(crate::i18n::Locale::English);
        assert_eq!(en_labels.nav_explore, "Explore");
        assert_eq!(en_labels.explore_loading, "Loading explore...");
        assert_eq!(en_labels.explore_empty, "No sections found in Explore.");

        let es_labels = StaticLabels::resolve(crate::i18n::Locale::Spanish);
        assert_eq!(es_labels.nav_explore, "Explorar");
        assert_eq!(es_labels.explore_loading, "Cargando exploracion...");
        assert_eq!(es_labels.explore_empty, "No se encontraron secciones en Explorar.");
    }

    #[test]
    fn test_static_labels_resolution_spanish_and_english() {
        let es = StaticLabels::resolve(Locale::Spanish);
        assert_eq!(es.nav_search, "Buscar");
        assert_eq!(es.nav_explore, "Explorar");
        assert_eq!(es.nav_library, "Biblioteca");
        assert_eq!(es.nav_now_playing, "Sonando");
        assert_eq!(es.search_placeholder, "Buscar canciones...");
        assert_eq!(es.search_button, "Buscar");
        assert_eq!(es.filter_all, "Todo");
        assert_eq!(es.filter_songs, "Canciones");
        assert_eq!(es.filter_albums, "Álbumes");
        assert_eq!(es.filter_artists, "Artistas");
        assert_eq!(es.filter_playlists, "Listas de reproducción");
        assert_eq!(es.search_empty, "No se encontraron resultados.");
        assert_eq!(es.queue_empty, "La cola está vacía.");
        assert_eq!(es.no_cover, "Sin portada");
        assert_eq!(es.lyrics_unavailable, "Letras no disponibles");
        assert_eq!(es.lyrics_hint, "Las letras sincronizadas aparecerán durante la reproducción");
        assert_eq!(
            es.session_anonymous_hint,
            "Sesión anónima: pegar cookies para desbloquear la biblioteca"
        );
        assert_eq!(es.session_cookie_placeholder, "Pegar cookies de sesión...");
        assert_eq!(es.sign_in, "Iniciar sesión");
        assert_eq!(es.session_signed_in_hint, "Sesión iniciada: biblioteca desbloqueada");
        assert_eq!(es.sign_out, "Cerrar sesión");
        assert_eq!(es.section_playlists, "Listas de reproducción");
        assert_eq!(es.empty_playlists, "No hay listas de reproducción.");
        assert_eq!(es.section_albums, "Álbumes");
        assert_eq!(es.empty_albums, "No hay álbumes.");
        assert_eq!(es.section_artists, "Artistas");
        assert_eq!(es.empty_artists, "No hay artistas.");
        assert_eq!(es.empty_detail, "No hay detalles disponibles.");
        assert_eq!(es.play_button, "Reproducir");
        assert_eq!(es.shuffle_button, "Aleatorio");
        assert_eq!(es.empty_songs, "No hay canciones disponibles.");
        assert_eq!(es.section_theme, "Tema");
        assert_eq!(es.theme_white, "Blanco");
        assert_eq!(es.theme_black_oled, "Negro OLED");
        assert_eq!(es.theme_mono, "Monocromo");
        assert_eq!(es.section_playback, "Reproducción");
        assert_eq!(es.playback_speed, "Velocidad de reproducción");
        assert_eq!(es.section_session, "Sesión");
        assert_eq!(es.status_signed_in, "Estado: Sesión iniciada");
        assert_eq!(es.status_anonymous, "Estado: Anónimo");
        assert_eq!(es.section_about, "Acerca de");
        assert_eq!(es.app_version, "Versión de la app");
        assert_eq!(es.update_status, "Estado de actualización");
        assert_eq!(es.explore_loading, "Cargando exploracion...");
        assert_eq!(es.explore_empty, "No se encontraron secciones en Explorar.");

        let en = StaticLabels::resolve(Locale::English);
        assert_eq!(en.nav_search, "Search");
        assert_eq!(en.nav_explore, "Explore");
        assert_eq!(en.nav_library, "Library");
        assert_eq!(en.nav_now_playing, "Now Playing");
        assert_eq!(en.search_placeholder, "Search songs...");
        assert_eq!(en.search_button, "Search");
        assert_eq!(en.filter_all, "All");
        assert_eq!(en.filter_songs, "Songs");
        assert_eq!(en.filter_albums, "Albums");
        assert_eq!(en.filter_artists, "Artists");
        assert_eq!(en.filter_playlists, "Playlists");
        assert_eq!(en.search_empty, "No results found.");
        assert_eq!(en.queue_empty, "The queue is empty.");
        assert_eq!(en.no_cover, "No cover");
        assert_eq!(en.lyrics_unavailable, "Lyrics unavailable");
        assert_eq!(en.lyrics_hint, "Synchronized lyrics will appear during playback");
        assert_eq!(en.session_anonymous_hint, "Anonymous session: paste cookies to unlock library");
        assert_eq!(en.session_cookie_placeholder, "Paste session cookies...");
        assert_eq!(en.sign_in, "Sign in");
        assert_eq!(en.session_signed_in_hint, "Signed in: library unlocked");
        assert_eq!(en.sign_out, "Sign out");
        assert_eq!(en.section_playlists, "Playlists");
        assert_eq!(en.empty_playlists, "No playlists found.");
        assert_eq!(en.section_albums, "Albums");
        assert_eq!(en.empty_albums, "No albums found.");
        assert_eq!(en.section_artists, "Artists");
        assert_eq!(en.empty_artists, "No artists found.");
        assert_eq!(en.empty_detail, "No details available.");
        assert_eq!(en.play_button, "Play");
        assert_eq!(en.shuffle_button, "Shuffle");
        assert_eq!(en.empty_songs, "No songs available.");
        assert_eq!(en.section_theme, "Theme");
        assert_eq!(en.theme_white, "White");
        assert_eq!(en.theme_black_oled, "Black OLED");
        assert_eq!(en.theme_mono, "Mono");
        assert_eq!(en.section_playback, "Playback");
        assert_eq!(en.playback_speed, "Playback speed");
        assert_eq!(en.section_session, "Session");
        assert_eq!(en.status_signed_in, "Status: Signed in");
        assert_eq!(en.status_anonymous, "Status: Anonymous");
        assert_eq!(en.section_about, "About");
        assert_eq!(en.app_version, "App version");
        assert_eq!(en.update_status, "Update status");
        assert_eq!(en.explore_loading, "Loading explore...");
        assert_eq!(en.explore_empty, "No sections found in Explore.");
    }
}
