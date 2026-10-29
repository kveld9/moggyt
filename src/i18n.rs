//! Internationalization (i18n) skeleton.
//!
//! Note: Phase 2 will transition to PO-based translation catalogs (fastframe-i18n style).
//! For tranche 1, translations are kept in a minimal, statically-typed in-memory lookup table.
//!
//! Exception: User-facing Spanish UI string literals are permitted in this module only.

use serde::{Deserialize, Serialize};

/// Supported interface languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Locale {
    /// English (default fallback).
    #[default]
    #[serde(rename = "en")]
    English,
    /// Spanish.
    #[serde(rename = "es")]
    Spanish,
}

impl Locale {
    /// Returns the IETF BCP 47 language tag ("en", "es").
    pub fn tag(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    /// Returns the native human-readable name of the language.
    pub fn native_name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }

    /// Parses a stored tag into a `Locale`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Self::English),
            "es" => Some(Self::Spanish),
            _ => None,
        }
    }

    /// Detects system language preference lightly from environment, defaulting to English.
    /// Under test configurations, always returns English for deterministic runs.
    pub fn detect() -> Self {
        if cfg!(test) {
            return Self::English;
        }
        detect_system_locale()
    }
}

fn detect_system_locale() -> Locale {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if let Some(loc) = parse_env_lang(&val) {
                return loc;
            }
        }
    }
    Locale::English
}

fn parse_env_lang(val: &str) -> Option<Locale> {
    let lower = val.to_ascii_lowercase();
    if lower.starts_with("es") {
        Some(Locale::Spanish)
    } else if lower.starts_with("en") {
        Some(Locale::English)
    } else {
        None
    }
}

/// Translates a UI key for the specified locale, falling back to the English source key.
pub fn tr(locale: Locale, key: &str) -> &str {
    match locale {
        Locale::English => key,
        Locale::Spanish => translate_spanish(key),
    }
}

/// Static lookup table of English source keys to Spanish UI strings.
/// Permitted exception: User-facing Spanish UI string literals are permitted in this table.
static SPANISH_TRANSLATIONS: &[(&str, &str)] = &[
    ("About", "Acerca de"),
    ("Album", "ÁLBUM"),
    ("Albums", "Álbumes"),
    ("All", "Todo"),
    (
        "Anonymous session: paste cookies to unlock library",
        "Sesión anónima: pegar cookies para desbloquear la biblioteca",
    ),
    ("App version", "Versión de la app"),
    ("Artist", "ARTISTA"),
    ("Artists", "Artistas"),
    ("Black OLED", "Negro OLED"),
    ("Detail", "Detalle"),
    ("English", "Inglés"),
    ("Failed to play track", "Error al reproducir la canción"),
    ("Language", "Idioma"),
    ("Library", "Biblioteca"),
    ("Lyrics unavailable", "Letras no disponibles"),
    ("Mono", "Monocromo"),
    ("No albums found.", "No hay álbumes."),
    ("No artists found.", "No hay artistas."),
    ("No cover", "Sin portada"),
    ("No details available.", "No hay detalles disponibles."),
    ("No playlists found.", "No hay listas de reproducción."),
    ("No results found.", "No se encontraron resultados."),
    ("No songs available.", "No hay canciones disponibles."),
    ("No track playing", "Sin reproducción"),
    ("No tracks available.", "No hay canciones disponibles."),
    ("Now Playing", "Sonando"),
    ("Paste session cookies...", "Pegar cookies de sesión..."),
    ("Play", "Reproducir"),
    ("Playback", "Reproducción"),
    ("Playback speed", "Velocidad de reproducción"),
    ("Playlist", "LISTA"),
    ("Playlists", "Listas de reproducción"),
    ("Queue", "Cola"),
    ("Search", "Buscar"),
    ("Search Results", "Resultados de búsqueda"),
    ("Search songs...", "Buscar canciones..."),
    ("Session", "Sesión"),
    ("Settings", "Ajustes"),
    ("Shuffle", "Aleatorio"),
    ("Sign in", "Iniciar sesión"),
    ("Sign out", "Cerrar sesión"),
    ("Signed in: library unlocked", "Sesión iniciada: biblioteca desbloqueada"),
    ("Songs", "Canciones"),
    ("Spanish", "Español"),
    ("Status: Anonymous", "Estado: Anónimo"),
    ("Status: Signed in", "Estado: Sesión iniciada"),
    (
        "Synchronized lyrics will appear during playback",
        "Las letras sincronizadas aparecerán durante la reproducción",
    ),
    ("The queue is empty.", "La cola está vacía."),
    ("Theme", "Tema"),
    ("Up to date", "Actualizado"),
    ("Update available", "Actualización disponible"),
    ("Update available: {ver}", "Actualización disponible: {ver}"),
    ("Update status", "Estado de actualización"),
    ("White", "Blanco"),
];

fn translate_spanish(key: &str) -> &str {
    SPANISH_TRANSLATIONS.iter().find(|&&(k, _)| k == key).map(|&(_, v)| v).unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_6_given_locale_spanish_when_tr_called_then_returns_spanish_and_fallbacks_to_english() {
        // 1. Spanish translations match expected strings
        for &(key, expected_es) in SPANISH_TRANSLATIONS {
            assert_eq!(
                tr(Locale::Spanish, key),
                expected_es,
                "expected Spanish translation for '{key}'"
            );
        }

        // 2. Unknown key in Spanish falls back to English source key
        assert_eq!(tr(Locale::Spanish, "nonexistent_key"), "nonexistent_key");

        // 3. English locale returns English source key as-is
        for &(key, _) in SPANISH_TRANSLATIONS {
            assert_eq!(tr(Locale::English, key), key);
        }
        assert_eq!(tr(Locale::English, "nonexistent_key"), "nonexistent_key");
    }

    #[test]
    fn test_ui_chrome_keys_and_parameterized_pattern() {
        // Page titles
        assert_eq!(tr(Locale::Spanish, "Search"), "Buscar");
        assert_eq!(tr(Locale::Spanish, "Queue"), "Cola");
        assert_eq!(tr(Locale::Spanish, "Now Playing"), "Sonando");
        assert_eq!(tr(Locale::Spanish, "Library"), "Biblioteca");
        assert_eq!(tr(Locale::Spanish, "Detail"), "Detalle");
        assert_eq!(tr(Locale::Spanish, "Settings"), "Ajustes");

        // Sentinels
        assert_eq!(tr(Locale::Spanish, "No track playing"), "Sin reproducción");
        assert_eq!(tr(Locale::Spanish, "Search Results"), "Resultados de búsqueda");

        // Update status & parameterized pattern
        assert_eq!(tr(Locale::Spanish, "Up to date"), "Actualizado");
        assert_eq!(tr(Locale::Spanish, "Update available"), "Actualización disponible");
        assert_eq!(
            tr(Locale::Spanish, "Update available: {ver}").replace("{ver}", "v1.2.3"),
            "Actualización disponible: v1.2.3"
        );
        assert_eq!(
            tr(Locale::English, "Update available: {ver}").replace("{ver}", "v1.2.3"),
            "Update available: v1.2.3"
        );

        // Kind labels
        assert_eq!(tr(Locale::Spanish, "Album"), "ÁLBUM");
        assert_eq!(tr(Locale::Spanish, "Artist"), "ARTISTA");
        assert_eq!(tr(Locale::Spanish, "Playlist"), "LISTA");
    }

    #[test]
    fn test_7_given_stored_tag_when_from_tag_parses_then_round_trips_via_tag_and_unknown_returns_none()
     {
        // 1. Valid tags parse correctly
        assert_eq!(Locale::from_tag("en"), Some(Locale::English));
        assert_eq!(Locale::from_tag("es"), Some(Locale::Spanish));

        // 2. Round-trip via tag()
        assert_eq!(Locale::from_tag(Locale::English.tag()), Some(Locale::English));
        assert_eq!(Locale::from_tag(Locale::Spanish.tag()), Some(Locale::Spanish));

        // 3. Native names
        assert_eq!(Locale::English.native_name(), "English");
        assert_eq!(Locale::Spanish.native_name(), "Español");

        // 4. Unknown/invalid tags return None
        assert_eq!(Locale::from_tag("fr"), None);
        assert_eq!(Locale::from_tag("de"), None);
        assert_eq!(Locale::from_tag(""), None);
        assert_eq!(Locale::from_tag("unknown"), None);
    }

    #[test]
    fn test_8_given_env_lang_strings_when_parse_env_lang_called_then_maps_expected_locales() {
        let cases = [
            ("es", Some(Locale::Spanish)),
            ("es_AR.UTF-8", Some(Locale::Spanish)),
            ("es-ES", Some(Locale::Spanish)),
            ("en", Some(Locale::English)),
            ("en_US.UTF-8", Some(Locale::English)),
            ("fr_FR", None),
            ("C", None),
            ("POSIX", None),
            ("", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_env_lang(input), expected, "failed for input: {input}");
        }
    }
}
