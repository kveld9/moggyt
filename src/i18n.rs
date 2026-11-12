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
    ("Accent color", "Color de acento"),
    ("Add to queue", "Añadir a la cola"),
    ("Added to favorites", "Añadido a favoritos"),
    ("Added to queue", "Añadido a la cola"),
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
    ("Autoplay similar tracks", "Reproducción automática de similares"),
    ("Black OLED", "Negro OLED"),
    ("Both Spotify credentials are required", "Se requieren ambas credenciales de Spotify"),
    ("CSV file path", "Ruta del archivo CSV"),
    ("Cancel", "Cancelar"),
    ("Category", "Categoria"),
    ("Choose a track", "Elegir una cancion"),
    ("Classic", "Clásica"),
    ("Clear credentials", "Borrar credenciales"),
    ("Clear history", "Borrar todo el historial"),
    ("Clear queue", "Vaciar cola"),
    ("Close", "Cerrar"),
    ("Connect with Last.fm", "Conectar con Last.fm"),
    ("Connected as", "Conectado como"),
    (
        "Connection lost. Retry or save the matched tracks.",
        "Se perdió la conexión. Reintenta o guarda las canciones coincidentes.",
    ),
    ("Copy link", "Copiar enlace"),
    ("Could not copy the link", "No se pudo copiar el enlace"),
    ("Crossfade", "Fundido cruzado"),
    ("Custom", "Personalizado"),
    ("Custom Discord Application ID", "ID de aplicación personalizada de Discord"),
    ("Custom theme loaded from {path}", "Tema personalizado cargado desde {path}"),
    ("Custom theme not loaded: {error}", "Tema personalizado no cargado: {error}"),
    ("Delete playlist", "Borrar playlist"),
    ("Detail", "Detalle"),
    ("Discard", "Descartar"),
    ("Disconnect", "Desconectar"),
    ("Disk space quota for offline stream cache", "Espacio en disco para la caché de audio"),
    ("Discord Rich Presence", "Discord Rich Presence"),
    ("Dismiss", "Descartar"),
    ("Down", "Bajar"),
    (
        "Embed imports are capped at 100 tracks. Export a CSV for longer playlists.",
        "Las importaciones por enlace se limitan a 100 canciones. Exporta un CSV para playlists mas largas.",
    ),
    ("English", "Inglés"),
    ("End of track", "Fin de la canción"),
    ("Explore", "Explorar"),
    ("Failed to clear Spotify credentials", "No se pudieron borrar las credenciales de Spotify"),
    ("Failed to load search results", "No se pudieron cargar los resultados de busqueda"),
    ("Failed to open browser", "No se pudo abrir el navegador"),
    ("Failed to play track", "Error al reproducir la canción"),
    ("Failed to save playlist", "No se pudo guardar la playlist"),
    ("Failed to save Spotify credentials", "No se pudieron guardar las credenciales de Spotify"),
    ("Fix audio version", "Corregir versión de audio"),
    ("Fix version", "Corregir versión"),
    ("Floating", "Flotante"),
    ("From file", "Desde archivo"),
    ("From link", "Desde enlace"),
    ("Go to album", "Ir al álbum"),
    ("Go to artist", "Ir al artista"),
    ("High (AAC max ~130 kbps)", "Alta (AAC máx ~130 kbps)"),
    ("High confidence", "Confianza alta"),
    ("History", "Historial"),
    ("Import", "Importar"),
    ("Import cancelled", "Importacion cancelada"),
    ("Import from Spotify", "Importar desde Spotify"),
    ("Import progress", "Progreso de importacion"),
    ("Importing matches...", "Importando coincidencias..."),
    ("Integrations", "Integraciones"),
    ("Invalid Spotify link", "Enlace de Spotify no valido"),
    ("Language", "Idioma"),
    ("Last 4 weeks", "Últimas 4 semanas"),
    ("Last 6 months", "Últimos 6 meses"),
    ("Last.fm connection failed", "Falló la conexión con Last.fm"),
    ("Last.fm is not available in this build", "Last.fm no está disponible en esta compilación"),
    ("Library", "Biblioteca"),
    ("Like", "Me gusta"),
    ("Link copied", "Enlace copiado"),
    ("Listening time", "Tiempo escuchado"),
    ("Load directly to queue", "Cargar directamente en la cola"),
    ("Loading...", "Cargando..."),
    ("Loading explore...", "Cargando exploracion..."),
    ("Low (<=64 kbps)", "Baja (<=64 kbps)"),
    ("Low confidence", "Confianza baja"),
    ("Lyrics", "Letras"),
    ("Lyrics unavailable", "Letras no disponibles"),
    ("Matched", "Coincidencias"),
    ("Matched track", "Cancion encontrada"),
    ("Matugen", "Matugen"),
    ("Matugen theme loaded from {path}", "Tema Matugen cargado desde {path}"),
    ("Matugen theme not loaded: {error}", "Tema Matugen no cargado: {error}"),
    ("Medium confidence", "Confianza media"),
    ("Mono", "Monocromo"),
    ("Needs review", "Requieren revision"),
    ("New playlist name", "Nuevo nombre de playlist"),
    ("No album found", "No se encontró el álbum"),
    ("No albums found.", "No hay álbumes."),
    ("No alternatives found", "No se encontraron alternativas"),
    ("No artist found", "No se encontró el artista"),
    ("No cover", "Sin portada"),
    ("No details available.", "No hay detalles disponibles."),
    ("No match", "Sin coincidencia"),
    ("No matched tracks to import", "No hay canciones coincidentes para importar"),
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
    ("Cover", "Portada"),
    ("Vinyl", "Vinilo"),
    ("Off", "Desactivado"),
    ("Open", "Abrir"),
    ("Open settings", "Abrir ajustes"),
    ("Paste session cookies...", "Pegar cookies de sesión..."),
    (
        "Place theme JSON files in {path}/themes/ or {path}/custom-theme.json",
        "Coloca archivos JSON de tema en {path}/themes/ o {path}/custom-theme.json",
    ),
    (
        "Place theme JSON files in the themes directory or custom-theme.json inside the configuration directory",
        "Coloca archivos JSON de tema en el directorio de temas o custom-theme.json dentro del directorio de configuración",
    ),
    ("Player bar", "Barra de reproducción"),
    ("Play", "Reproducir"),
    ("Play next", "Reproducir a continuación"),
    ("Popular songs", "Canciones populares"),
    ("Playback", "Reproducción"),
    ("Playback speed", "Velocidad de reproducción"),
    ("Playing next", "Se reproducirá a continuación"),
    ("Playlist", "LISTA"),
    ("Playlist name (optional)", "Nombre de la playlist (opcional)"),
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
    ("Remove", "Quitar"),
    ("Removed from favorites", "Quitado de favoritos"),
    ("Rename", "Renombrar"),
    ("Reset", "Restablecer"),
    ("Retry", "Reintentar"),
    ("Review matches", "Revisar coincidencias"),
    ("Save credentials", "Guardar credenciales"),
    ("Save playlist", "Guardar playlist"),
    ("Saved playlist", "Playlist guardada"),
    ("Search", "Buscar"),
    ("Search Results", "Resultados de búsqueda"),
    ("Search alternatives", "Buscar alternativas"),
    ("Search for a different recording", "Buscar otra grabación"),
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
    ("Show all", "Mostrar todo"),
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
    ("Song", "CANCIÓN"),
    ("Songs", "Canciones"),
    ("Source", "Origen"),
    ("Spanish", "Español"),
    ("Spotify Client ID", "ID de cliente de Spotify"),
    ("Spotify Client Secret", "Secreto de cliente de Spotify"),
    ("Spotify playlist or album link", "Enlace de playlist o album de Spotify"),
    ("Spotify Web API", "API web de Spotify"),
    ("Spotify Web API: configured", "API web de Spotify: configurada"),
    ("Spotify Web API: not configured", "API web de Spotify: no configurada"),
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
    ("Top result", "Resultado principal"),
    ("Top tracks", "Temas más escuchados"),
    ("Tracks", "Temas"),
    ("Unlike", "Quitar me gusta"),
    ("Unresolved", "Sin resolver"),
    ("Up", "Subir"),
    ("Up next", "A continuacion"),
    ("Up to date", "Actualizado"),
    ("Update available", "Actualización disponible"),
    ("Update available: {ver}", "Actualización disponible: {ver}"),
    ("Update status", "Estado de actualización"),
    ("Use ID", "Usar ID"),
    ("Valid Spotify link", "Enlace de Spotify valido"),
    ("Waiting for authorization in your browser", "Esperando la autorización en tu navegador"),
    ("White", "Blanco"),
    ("YouTube video ID", "ID de video de YouTube"),
    ("plays", "reproducciones"),
];

fn translate_spanish(key: &str) -> &str {
    SPANISH_TRANSLATIONS.iter().find(|&&(k, _)| k == key).map(|&(_, v)| v).unwrap_or(key)
}

#[cfg(test)]
#[path = "i18n_tests.rs"]
mod tests;
