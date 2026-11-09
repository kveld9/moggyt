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
    ("15m", "15 min"),
    ("30m", "30 min"),
    ("45m", "45 min"),
    ("60m", "60 min"),
    ("About", "Acerca de"),
    ("Album", "ÁLBUM"),
    ("Albums", "Álbumes"),
    ("All", "Todo"),
    ("All matched", "Todo coincidio"),
    ("All time", "Todo el tiempo"),
    (
        "Anonymous session: paste cookies to unlock library",
        "Sesión anónima: pegar cookies para desbloquear la biblioteca",
    ),
    ("App version", "Versión de la app"),
    ("Applies from the next track", "Se aplica desde la siguiente canción"),
    ("Artist", "ARTISTA"),
    ("Artists", "Artistas"),
    ("Audio quality", "Calidad de audio"),
    ("Black OLED", "Negro OLED"),
    ("CSV file path", "Ruta del archivo CSV"),
    ("Cancel", "Cancelar"),
    ("Category", "Categoria"),
    ("Choose a track", "Elegir una cancion"),
    ("Classic", "Clásica"),
    ("Clear history", "Borrar todo el historial"),
    ("Close", "Cerrar"),
    ("Connect with Last.fm", "Conectar con Last.fm"),
    ("Connected as", "Conectado como"),
    ("Crossfade", "Fundido cruzado"),
    ("Custom", "Personalizado"),
    ("Custom Discord Application ID", "ID de aplicación personalizada de Discord"),
    ("Custom theme loaded from {path}", "Tema personalizado cargado desde {path}"),
    ("Custom theme not loaded: {error}", "Tema personalizado no cargado: {error}"),
    ("Detail", "Detalle"),
    ("Discard", "Descartar"),
    ("Disconnect", "Desconectar"),
    ("Disk space quota for offline stream cache", "Espacio en disco para la caché de audio"),
    ("Discord Rich Presence", "Discord Rich Presence"),
    ("Dismiss", "Descartar"),
    (
        "Embed imports are capped at 100 tracks. Export a CSV for longer playlists.",
        "Las importaciones por enlace se limitan a 100 canciones. Exporta un CSV para playlists mas largas.",
    ),
    ("English", "Inglés"),
    ("End of track", "Fin de la canción"),
    ("Explore", "Explorar"),
    ("Failed to load search results", "No se pudieron cargar los resultados de busqueda"),
    ("Failed to play track", "Error al reproducir la canción"),
    ("Failed to save playlist", "No se pudo guardar la playlist"),
    ("Floating", "Flotante"),
    ("From file", "Desde archivo"),
    ("From link", "Desde enlace"),
    ("High (>=192 kbps)", "Alta (>=192 kbps)"),
    ("High confidence", "Confianza alta"),
    ("History", "Historial"),
    ("Import", "Importar"),
    ("Import cancelled", "Importacion cancelada"),
    ("Import from Spotify", "Importar desde Spotify"),
    ("Import progress", "Progreso de importacion"),
    ("Importing matches...", "Importando coincidencias..."),
    ("Integrations", "Integraciones"),
    ("Language", "Idioma"),
    ("Last 4 weeks", "Últimas 4 semanas"),
    ("Last 6 months", "Últimos 6 meses"),
    ("Last.fm connection failed", "Falló la conexión con Last.fm"),
    ("Last.fm is not available in this build", "Last.fm no está disponible en esta compilación"),
    ("Library", "Biblioteca"),
    ("Listening time", "Tiempo escuchado"),
    ("Loading...", "Cargando..."),
    ("Loading explore...", "Cargando exploracion..."),
    ("Low (<=64 kbps)", "Baja (<=64 kbps)"),
    ("Low confidence", "Confianza baja"),
    ("Lyrics unavailable", "Letras no disponibles"),
    ("Matched", "Coincidencias"),
    ("Matched track", "Cancion encontrada"),
    ("Medium confidence", "Confianza media"),
    ("Mono", "Monocromo"),
    ("Needs review", "Requieren revision"),
    ("No albums found.", "No hay álbumes."),
    ("No cover", "Sin portada"),
    ("No details available.", "No hay detalles disponibles."),
    ("No match", "Sin coincidencia"),
    (
        "No plays recorded yet. Songs you listen to for at least 30 seconds show up here.",
        "Todavía no hay reproducciones. Los temas que escuches al menos 30 segundos aparecen acá.",
    ),
    ("No playlist store installed", "No hay almacen de playlists instalado"),
    ("No playlists found.", "No hay listas de reproducción."),
    ("No recent searches.", "Aún no hay búsquedas recientes."),
    ("No results found.", "No se encontraron resultados."),
    ("No sections found in Explore.", "No se encontraron secciones en Explorar."),
    ("No songs available.", "No hay canciones disponibles."),
    ("No track playing", "Sin reproducción"),
    ("No tracks available.", "No hay canciones disponibles."),
    ("Not connected", "No conectado"),
    ("Now Playing", "Sonando"),
    ("Off", "Desactivado"),
    ("Open settings", "Abrir ajustes"),
    ("Paste session cookies...", "Pegar cookies de sesión..."),
    (
        "Place theme JSON files in ~/.config/moggyt/themes/ or ~/.config/moggyt/custom-theme.json",
        "Coloca archivos JSON de tema en ~/.config/moggyt/themes/ o ~/.config/moggyt/custom-theme.json",
    ),
    ("Player bar", "Barra de reproducción"),
    ("Play", "Reproducir"),
    ("Playback", "Reproducción"),
    ("Playback speed", "Velocidad de reproducción"),
    ("Playlist", "LISTA"),
    ("Playlists", "Listas de reproducción"),
    ("Plays", "Reproducciones"),
    ("Prefer songs over music videos", "Preferir canciones a videoclips"),
    ("Queue", "Cola"),
    ("Queue ({count})", "Cola ({count})"),
    ("Queue file uses a newer schema", "El archivo de cola usa un esquema mas nuevo"),
    (
        "Queue file was corrupt and was quarantined",
        "El archivo de cola estaba corrupto y se puso en cuarentena",
    ),
    ("Recent searches", "Búsquedas recientes"),
    ("Review matches", "Revisar coincidencias"),
    ("Save playlist", "Guardar playlist"),
    ("Saved playlist", "Playlist guardada"),
    ("Search", "Buscar"),
    ("Search Results", "Resultados de búsqueda"),
    ("Search songs...", "Buscar canciones..."),
    ("Searching...", "Buscando..."),
    ("Select a playlist file", "Selecciona un archivo de playlist"),
    ("Session", "Sesión"),
    ("Settings", "Ajustes"),
    ("Settings file uses a newer schema", "El archivo de ajustes usa un esquema mas nuevo"),
    (
        "Settings file was corrupt and was quarantined",
        "El archivo de ajustes estaba corrupto y se puso en cuarentena",
    ),
    ("Show artwork", "Mostrar portada"),
    ("Show like button", "Mostrar botón Me gusta"),
    ("Show queue button", "Mostrar botón de cola"),
    ("Show repeat button", "Mostrar botón Repetir"),
    ("Show shuffle button", "Mostrar botón Aleatorio"),
    ("Show skip 10s buttons", "Mostrar botones +/-10s"),
    ("Show volume slider", "Mostrar control de volumen"),
    (
        "Shows the current track in your Discord status while Discord is running.",
        "Muestra el tema actual en tu estado de Discord mientras Discord está abierto.",
    ),
    ("Shuffle", "Aleatorio"),
    ("Sign in", "Iniciar sesión"),
    (
        "Sign in from Settings to load your library.",
        "Inicia sesión desde Ajustes para cargar tu biblioteca.",
    ),
    ("Sign out", "Cerrar sesión"),
    ("Signed in: library unlocked", "Sesión iniciada: biblioteca desbloqueada"),
    ("Sleep timer", "Temporizador de apagado"),
    ("Smooth transition between tracks", "Transición suave entre temas"),
    ("Songs", "Canciones"),
    ("Source", "Origen"),
    ("Spanish", "Español"),
    ("Spotify playlist or album link", "Enlace de playlist o album de Spotify"),
    ("Start import", "Iniciar importacion"),
    ("Standard (~128 kbps)", "Estándar (~128 kbps)"),
    ("Stats", "Estadísticas"),
    ("Status: Anonymous", "Estado: Anónimo"),
    ("Status: Signed in", "Estado: Sesión iniciada"),
    ("Stream Cache Limit", "Límite de caché de streams"),
    ("Swap", "Cambiar"),
    (
        "Synchronized lyrics will appear during playback",
        "Las letras sincronizadas aparecerán durante la reproducción",
    ),
    ("The queue is empty.", "La cola está vacía."),
    ("Theme", "Tema"),
    ("Timer:", "Temporizador:"),
    ("Timer: End of track", "Temporizador: Fin de la canción"),
    ("Timer: Off", "Temporizador: Desactivado"),
    ("Timer: {time}", "Temporizador: {time}"),
    ("Top artists", "Artistas más escuchados"),
    ("Top tracks", "Temas más escuchados"),
    ("Tracks", "Temas"),
    ("Unresolved", "Sin resolver"),
    ("Up to date", "Actualizado"),
    ("Update available", "Actualización disponible"),
    ("Update available: {ver}", "Actualización disponible: {ver}"),
    ("Update status", "Estado de actualización"),
    ("Waiting for authorization in your browser", "Esperando la autorización en tu navegador"),
    ("White", "Blanco"),
    ("plays", "reproducciones"),
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
        assert_eq!(tr(Locale::Spanish, "History"), "Historial");
        assert_eq!(tr(Locale::Spanish, "Detail"), "Detalle");
        assert_eq!(tr(Locale::Spanish, "Settings"), "Ajustes");
        assert_eq!(tr(Locale::Spanish, "Explore"), "Explorar");
        assert_eq!(tr(Locale::Spanish, "Stats"), "Estadísticas");

        // Sentinels
        assert_eq!(tr(Locale::Spanish, "No track playing"), "Sin reproducción");
        assert_eq!(tr(Locale::Spanish, "Search Results"), "Resultados de búsqueda");
        assert_eq!(tr(Locale::Spanish, "Recent searches"), "Búsquedas recientes");
        assert_eq!(tr(Locale::Spanish, "No recent searches."), "Aún no hay búsquedas recientes.");
        assert_eq!(tr(Locale::Spanish, "Clear history"), "Borrar todo el historial");
        assert_eq!(
            tr(Locale::Spanish, "No sections found in Explore."),
            "No se encontraron secciones en Explorar."
        );

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

        // Custom theme status & parameterized pattern
        assert_eq!(tr(Locale::Spanish, "Custom"), "Personalizado");
        assert_eq!(tr(Locale::English, "Custom"), "Custom");
        assert_eq!(
            tr(Locale::Spanish, "Custom theme loaded from {path}")
                .replace("{path}", "/tmp/theme.json"),
            "Tema personalizado cargado desde /tmp/theme.json"
        );
        assert_eq!(
            tr(Locale::English, "Custom theme loaded from {path}")
                .replace("{path}", "/tmp/theme.json"),
            "Custom theme loaded from /tmp/theme.json"
        );
        assert_eq!(
            tr(Locale::Spanish, "Custom theme not loaded: {error}")
                .replace("{error}", "file not found"),
            "Tema personalizado no cargado: file not found"
        );
        assert_eq!(
            tr(Locale::English, "Custom theme not loaded: {error}")
                .replace("{error}", "file not found"),
            "Custom theme not loaded: file not found"
        );

        // Kind labels
        assert_eq!(tr(Locale::Spanish, "Album"), "ÁLBUM");
        assert_eq!(tr(Locale::Spanish, "Artist"), "ARTISTA");
        assert_eq!(tr(Locale::Spanish, "Playlist"), "LISTA");

        // Dynamic labels & transient status
        assert_eq!(tr(Locale::Spanish, "Queue ({count})").replace("{count}", "3"), "Cola (3)");
        assert_eq!(tr(Locale::English, "Queue ({count})").replace("{count}", "3"), "Queue (3)");
        assert_eq!(tr(Locale::Spanish, "Searching..."), "Buscando...");
        assert_eq!(tr(Locale::English, "Searching..."), "Searching...");
        assert_eq!(
            tr(Locale::English, "Failed to load search results"),
            "Failed to load search results"
        );
        assert_eq!(
            tr(Locale::Spanish, "Failed to load search results"),
            "No se pudieron cargar los resultados de busqueda"
        );
        assert_eq!(tr(Locale::Spanish, "Loading..."), "Cargando...");
        assert_eq!(tr(Locale::English, "Loading..."), "Loading...");
        assert_eq!(tr(Locale::Spanish, "Loading explore..."), "Cargando exploracion...");
        assert_eq!(tr(Locale::English, "Loading explore..."), "Loading explore...");

        // Player bar chrome keys
        assert_eq!(tr(Locale::Spanish, "Player bar"), "Barra de reproducción");
        assert_eq!(tr(Locale::Spanish, "Classic"), "Clásica");
        assert_eq!(tr(Locale::Spanish, "Floating"), "Flotante");
        assert_eq!(tr(Locale::Spanish, "Show artwork"), "Mostrar portada");
        assert_eq!(tr(Locale::Spanish, "Show like button"), "Mostrar botón Me gusta");
        assert_eq!(tr(Locale::Spanish, "Show shuffle button"), "Mostrar botón Aleatorio");
        assert_eq!(tr(Locale::Spanish, "Show repeat button"), "Mostrar botón Repetir");
        assert_eq!(tr(Locale::Spanish, "Show skip 10s buttons"), "Mostrar botones +/-10s");
        assert_eq!(tr(Locale::Spanish, "Show queue button"), "Mostrar botón de cola");
        assert_eq!(tr(Locale::Spanish, "Show volume slider"), "Mostrar control de volumen");
        assert_eq!(
            tr(Locale::Spanish, "Prefer songs over music videos"),
            "Preferir canciones a videoclips"
        );

        // Stats page labels
        assert_eq!(tr(Locale::Spanish, "Last 4 weeks"), "Últimas 4 semanas");
        assert_eq!(tr(Locale::Spanish, "Last 6 months"), "Últimos 6 meses");
        assert_eq!(tr(Locale::Spanish, "All time"), "Todo el tiempo");
        assert_eq!(tr(Locale::Spanish, "Plays"), "Reproducciones");
        assert_eq!(tr(Locale::Spanish, "Listening time"), "Tiempo escuchado");
        assert_eq!(tr(Locale::Spanish, "Tracks"), "Temas");
        assert_eq!(tr(Locale::Spanish, "Artists"), "Artistas");
        assert_eq!(tr(Locale::Spanish, "Top artists"), "Artistas más escuchados");
        assert_eq!(tr(Locale::Spanish, "Top tracks"), "Temas más escuchados");
        assert_eq!(tr(Locale::Spanish, "plays"), "reproducciones");
        assert_eq!(
            tr(
                Locale::Spanish,
                "No plays recorded yet. Songs you listen to for at least 30 seconds show up here."
            ),
            "Todavía no hay reproducciones. Los temas que escuches al menos 30 segundos aparecen acá."
        );
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

    #[test]
    fn test_sleep_timer_i18n_strings_exist_and_differ() {
        let keys = [
            "Sleep timer",
            "Off",
            "15m",
            "30m",
            "45m",
            "60m",
            "End of track",
            "Timer:",
            "Timer: Off",
            "Timer: End of track",
            "Timer: {time}",
        ];
        for key in keys {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_audio_quality_i18n_strings_exist_and_differ() {
        let keys = [
            "Audio quality",
            "Applies from the next track",
            "Low (<=64 kbps)",
            "Standard (~128 kbps)",
            "High (>=192 kbps)",
        ];
        for key in keys {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_spotify_import_i18n_strings_exist_and_differ() {
        let keys_and_expected = [
            ("Import from Spotify", "Importar desde Spotify"),
            ("Import", "Importar"),
            ("Import cancelled", "Importacion cancelada"),
            ("From link", "Desde enlace"),
            ("From file", "Desde archivo"),
            ("Spotify playlist or album link", "Enlace de playlist o album de Spotify"),
            ("CSV file path", "Ruta del archivo CSV"),
            ("Start import", "Iniciar importacion"),
            ("Cancel", "Cancelar"),
            ("Importing matches...", "Importando coincidencias..."),
            ("Matched", "Coincidencias"),
            ("Needs review", "Requieren revision"),
            ("Unresolved", "Sin resolver"),
            ("Import progress", "Progreso de importacion"),
            (
                "Embed imports are capped at 100 tracks. Export a CSV for longer playlists.",
                "Las importaciones por enlace se limitan a 100 canciones. Exporta un CSV para playlists mas largas.",
            ),
            ("Close", "Cerrar"),
            ("All matched", "Todo coincidio"),
            ("Choose a track", "Elegir una cancion"),
            ("Discard", "Descartar"),
            ("High confidence", "Confianza alta"),
            ("Low confidence", "Confianza baja"),
            ("Matched track", "Cancion encontrada"),
            ("Medium confidence", "Confianza media"),
            ("No match", "Sin coincidencia"),
            ("No playlist store installed", "No hay almacen de playlists instalado"),
            ("Review matches", "Revisar coincidencias"),
            ("Save playlist", "Guardar playlist"),
            ("Saved playlist", "Playlist guardada"),
            ("Failed to save playlist", "No se pudo guardar la playlist"),
            ("Select a playlist file", "Selecciona un archivo de playlist"),
            ("Source", "Origen"),
            ("Swap", "Cambiar"),
        ];

        for (key, expected_es) in keys_and_expected {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_eq!(es, expected_es, "Spanish must match expected translation for '{key}'");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_lastfm_i18n_strings_exist_and_differ() {
        let keys_and_expected = [
            ("Connect with Last.fm", "Conectar con Last.fm"),
            ("Connected as", "Conectado como"),
            ("Disconnect", "Desconectar"),
            ("Last.fm connection failed", "Falló la conexión con Last.fm"),
            (
                "Last.fm is not available in this build",
                "Last.fm no está disponible en esta compilación",
            ),
            ("Not connected", "No conectado"),
            (
                "Waiting for authorization in your browser",
                "Esperando la autorización en tu navegador",
            ),
        ];

        for (key, expected_es) in keys_and_expected {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_eq!(es, expected_es, "Spanish must match expected translation for '{key}'");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_crossfade_and_stream_cache_i18n_strings_exist_and_differ() {
        let keys_and_expected = [
            ("Crossfade", "Fundido cruzado"),
            ("Smooth transition between tracks", "Transición suave entre temas"),
            ("Stream Cache Limit", "Límite de caché de streams"),
            (
                "Disk space quota for offline stream cache",
                "Espacio en disco para la caché de audio",
            ),
        ];

        for (key, expected_es) in keys_and_expected {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_eq!(es, expected_es, "Spanish must match expected translation for '{key}'");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_library_view_i18n_strings_exist_and_differ() {
        let keys_and_expected = [
            ("Open settings", "Abrir ajustes"),
            (
                "Sign in from Settings to load your library.",
                "Inicia sesión desde Ajustes para cargar tu biblioteca.",
            ),
        ];

        for (key, expected_es) in keys_and_expected {
            let en = tr(Locale::English, key);
            let es = tr(Locale::Spanish, key);
            assert_eq!(en, key, "English must return source key");
            assert_eq!(es, expected_es, "Spanish must match expected translation for '{key}'");
            assert_ne!(en, es, "Spanish must differ from English for '{key}'");
        }
    }

    #[test]
    fn test_tr_spanish_translations_exhaustive() {
        let key = "Place theme JSON files in ~/.config/moggyt/themes/ or ~/.config/moggyt/custom-theme.json";
        let expected = "Coloca archivos JSON de tema en ~/.config/moggyt/themes/ o ~/.config/moggyt/custom-theme.json";
        assert_eq!(tr(Locale::English, key), key);
        assert_eq!(tr(Locale::Spanish, key), expected);
    }
}
