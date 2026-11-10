#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::i18n::Locale;
    use crate::slint_shell::labels::StaticLabels;

    #[test]
    fn test_static_labels_resolution_and_sync() {
        let en_labels = StaticLabels::resolve(crate::i18n::Locale::English);
        assert_eq!(en_labels.search_loading, "Searching...");
        assert_eq!(en_labels.search_error, "Failed to load search results");
        let es_labels = StaticLabels::resolve(crate::i18n::Locale::Spanish);
        assert_eq!(es_labels.search_loading, "Buscando...");
        assert_eq!(es_labels.search_error, "No se pudieron cargar los resultados de busqueda");
        assert_eq!(en_labels.nav_explore, "Explore");
        assert_eq!(en_labels.explore_loading, "Loading explore...");
        assert_eq!(en_labels.explore_empty, "No sections found in Explore.");
        assert_eq!(en_labels.section_integrations, "Integrations");
        assert_eq!(en_labels.discord_presence_label, "Discord Rich Presence");
        assert_eq!(
            en_labels.discord_presence_hint_label,
            "Shows the current track in your Discord status while Discord is running."
        );
        assert_eq!(en_labels.lastfm_label, "Last.fm");
        assert_eq!(en_labels.lastfm_connect_label, "Connect with Last.fm");
        assert_eq!(en_labels.lastfm_disconnect_label, "Disconnect");
        assert_eq!(en_labels.spotify_label, "Spotify Web API");
        assert_eq!(en_labels.spotify_client_id_placeholder, "Spotify Client ID");
        assert_eq!(en_labels.spotify_client_secret_placeholder, "Spotify Client Secret");
        assert_eq!(en_labels.spotify_save_label, "Save credentials");
        assert_eq!(en_labels.spotify_clear_label, "Clear credentials");
        assert_eq!(en_labels.import_name_placeholder, "Playlist name (optional)");
        assert_eq!(en_labels.import_to_queue_label, "Load directly to queue");
        assert_eq!(en_labels.import_valid_link_label, "Valid Spotify link");
        assert_eq!(en_labels.import_invalid_link_label, "Invalid Spotify link");

        let es_labels = StaticLabels::resolve(crate::i18n::Locale::Spanish);
        assert_eq!(es_labels.nav_explore, "Explorar");
        assert_eq!(es_labels.explore_loading, "Cargando exploracion...");
        assert_eq!(es_labels.explore_empty, "No se encontraron secciones en Explorar.");
        assert_eq!(es_labels.section_integrations, "Integraciones");
        assert_eq!(es_labels.discord_presence_label, "Discord Rich Presence");
        assert_eq!(
            es_labels.discord_presence_hint_label,
            "Muestra el tema actual en tu estado de Discord mientras Discord está abierto."
        );
        assert_eq!(es_labels.lastfm_label, "Last.fm");
        assert_eq!(es_labels.lastfm_connect_label, "Conectar con Last.fm");
        assert_eq!(es_labels.lastfm_disconnect_label, "Desconectar");
        assert_eq!(es_labels.spotify_label, "API web de Spotify");
        assert_eq!(es_labels.spotify_client_id_placeholder, "ID de cliente de Spotify");
        assert_eq!(es_labels.spotify_client_secret_placeholder, "Secreto de cliente de Spotify");
        assert_eq!(es_labels.spotify_save_label, "Guardar credenciales");
        assert_eq!(es_labels.spotify_clear_label, "Borrar credenciales");
        assert_eq!(es_labels.import_name_placeholder, "Nombre de la playlist (opcional)");
        assert_eq!(es_labels.import_to_queue_label, "Cargar directamente en la cola");
        assert_eq!(es_labels.import_valid_link_label, "Enlace de Spotify valido");
        assert_eq!(es_labels.import_invalid_link_label, "Enlace de Spotify no valido");
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
        assert_eq!(es.recent_searches_title, "Búsquedas recientes");
        assert_eq!(es.recent_searches_empty, "Aún no hay búsquedas recientes.");
        assert_eq!(es.clear_history, "Borrar todo el historial");
        assert_eq!(es.queue_title, "Cola");
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
        assert_eq!(
            es.library_signed_out_hint,
            "Inicia sesión desde Ajustes para cargar tu biblioteca."
        );
        assert_eq!(es.open_settings_label, "Abrir ajustes");
        assert_eq!(es.section_playlists, "Listas de reproducción");
        assert_eq!(es.empty_playlists, "No hay listas de reproducción.");
        assert_eq!(es.section_albums, "Álbumes");
        assert_eq!(es.empty_albums, "No hay álbumes.");
        assert_eq!(es.empty_detail, "No hay detalles disponibles.");
        assert_eq!(es.play_button, "Reproducir");
        assert_eq!(es.shuffle_button, "Aleatorio");
        assert_eq!(es.empty_songs, "No hay canciones disponibles.");
        assert_eq!(es.section_theme, "Tema");
        assert_eq!(es.theme_white, "Blanco");
        assert_eq!(es.theme_black_oled, "Negro OLED");
        assert_eq!(es.theme_mono, "Monocromo");
        assert_eq!(es.theme_custom, "Personalizado");
        assert_eq!(
            es.custom_theme_empty_hint,
            "Coloca archivos JSON de tema en ~/.config/moggyt/themes/ o ~/.config/moggyt/custom-theme.json"
        );
        assert_eq!(es.section_playback, "Reproducción");
        assert_eq!(es.playback_speed, "Velocidad de reproducción");
        assert_eq!(es.sleep_timer_label, "Temporizador de apagado");
        assert_eq!(es.sleep_timer_off_label, "Desactivado");
        assert_eq!(es.sleep_timer_15m_label, "15 min");
        assert_eq!(es.sleep_timer_30m_label, "30 min");
        assert_eq!(es.sleep_timer_45m_label, "45 min");
        assert_eq!(es.sleep_timer_60m_label, "60 min");
        assert_eq!(es.sleep_timer_end_of_track_label, "Fin de la canción");
        assert_eq!(es.audio_quality_label, "Calidad de audio");
        assert_eq!(es.audio_quality_hint_label, "Se aplica desde la siguiente canción");
        assert_eq!(es.audio_quality_low_label, "Baja (<=64 kbps)");
        assert_eq!(es.audio_quality_standard_label, "Estándar (~128 kbps)");
        assert_eq!(es.audio_quality_high_label, "Alta (>=192 kbps)");
        assert_eq!(es.songs_only_label, "Preferir canciones a videoclips");
        assert_eq!(es.crossfade, "Fundido cruzado");
        assert_eq!(es.crossfade_hint, "Transición suave entre temas");
        assert_eq!(es.stream_cache, "Límite de caché de streams");
        assert_eq!(es.stream_cache_hint, "Espacio en disco para la caché de audio");
        assert_eq!(es.section_integrations, "Integraciones");
        assert_eq!(es.discord_presence_label, "Discord Rich Presence");
        assert_eq!(
            es.discord_presence_hint_label,
            "Muestra el tema actual en tu estado de Discord mientras Discord está abierto."
        );
        assert_eq!(es.spotify_label, "API web de Spotify");
        assert_eq!(es.spotify_client_id_placeholder, "ID de cliente de Spotify");
        assert_eq!(es.spotify_client_secret_placeholder, "Secreto de cliente de Spotify");
        assert_eq!(es.spotify_save_label, "Guardar credenciales");
        assert_eq!(es.spotify_clear_label, "Borrar credenciales");
        assert_eq!(es.section_session, "Sesión");
        assert_eq!(es.status_signed_in, "Estado: Sesión iniciada");
        assert_eq!(es.status_anonymous, "Estado: Anónimo");
        assert_eq!(es.section_about, "Acerca de");
        assert_eq!(es.app_version, "Versión de la app");
        assert_eq!(es.update_status, "Estado de actualización");
        assert_eq!(es.explore_loading, "Cargando exploracion...");
        assert_eq!(es.explore_empty, "No se encontraron secciones en Explorar.");
        assert_eq!(es.section_player_bar, "Barra de reproducción");
        assert_eq!(es.player_bar_classic, "Clásica");
        assert_eq!(es.player_bar_floating, "Flotante");
        assert_eq!(es.player_bar_artwork, "Mostrar portada");
        assert_eq!(es.player_bar_like, "Mostrar botón Me gusta");
        assert_eq!(es.player_bar_shuffle, "Mostrar botón Aleatorio");
        assert_eq!(es.player_bar_repeat, "Mostrar botón Repetir");
        assert_eq!(es.player_bar_skip, "Mostrar botones +/-10s");
        assert_eq!(es.player_bar_queue, "Mostrar botón de cola");
        assert_eq!(es.player_bar_volume, "Mostrar control de volumen");
        assert_eq!(es.import_title_label, "Importar desde Spotify");
        assert_eq!(es.import_button_label, "Importar");
        assert_eq!(es.import_tab_link_label, "Desde enlace");
        assert_eq!(es.import_tab_file_label, "Desde archivo");
        assert_eq!(es.import_link_placeholder, "Enlace de playlist o album de Spotify");
        assert_eq!(es.import_file_placeholder, "Ruta del archivo CSV");
        assert_eq!(es.import_name_placeholder, "Nombre de la playlist (opcional)");
        assert_eq!(es.import_to_queue_label, "Cargar directamente en la cola");
        assert_eq!(es.import_valid_link_label, "Enlace de Spotify valido");
        assert_eq!(es.import_invalid_link_label, "Enlace de Spotify no valido");
        assert_eq!(es.import_start_label, "Iniciar importacion");
        assert_eq!(es.import_cancel_label, "Cancelar");
        assert_eq!(es.import_progress_title, "Progreso de importacion");
        assert_eq!(es.import_matched_label, "Coincidencias");
        assert_eq!(es.import_review_label, "Requieren revision");
        assert_eq!(es.import_unresolved_label, "Sin resolver");
        assert_eq!(
            es.import_cap_hint,
            "Las importaciones por enlace se limitan a 100 canciones. Exporta un CSV para playlists mas largas."
        );
        assert_eq!(es.import_close_label, "Cerrar");
        assert_eq!(es.import_review_title, "Revisar coincidencias");
        assert_eq!(es.import_source_label, "Origen");
        assert_eq!(es.import_matched_track_label, "Cancion encontrada");
        assert_eq!(es.import_high_confidence_label, "Confianza alta");
        assert_eq!(es.import_medium_confidence_label, "Confianza media");
        assert_eq!(es.import_low_confidence_label, "Confianza baja");
        assert_eq!(es.import_no_match_label, "Sin coincidencia");
        assert_eq!(es.import_swap_label, "Cambiar");
        assert_eq!(es.import_choose_track_label, "Elegir una cancion");
        assert_eq!(es.import_save_label, "Guardar playlist");
        assert_eq!(es.import_discard_label, "Descartar");
        assert_eq!(es.import_all_matched_label, "Todo coincidio");
        assert_eq!(es.import_custom_id_placeholder, "ID de video de YouTube");
        assert_eq!(es.import_use_id_label, "Usar ID");
        assert_eq!(es.nav_stats, "Estadísticas");
        assert_eq!(es.stats_last_4_weeks, "Últimas 4 semanas");
        assert_eq!(es.stats_last_6_months, "Últimos 6 meses");
        assert_eq!(es.stats_all_time, "Todo el tiempo");
        assert_eq!(es.stats_plays, "Reproducciones");
        assert_eq!(es.stats_listening_time, "Tiempo escuchado");
        assert_eq!(es.stats_tracks, "Temas");
        assert_eq!(es.stats_artists, "Artistas");
        assert_eq!(es.stats_top_artists, "Artistas más escuchados");
        assert_eq!(es.stats_top_tracks, "Temas más escuchados");
        assert_eq!(
            es.stats_empty,
            "Todavía no hay reproducciones. Los temas que escuches al menos 30 segundos aparecen acá."
        );

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
        assert_eq!(en.recent_searches_title, "Recent searches");
        assert_eq!(en.recent_searches_empty, "No recent searches.");
        assert_eq!(en.clear_history, "Clear history");
        assert_eq!(en.queue_title, "Queue");
        assert_eq!(en.queue_empty, "The queue is empty.");
        assert_eq!(en.no_cover, "No cover");
        assert_eq!(en.lyrics_unavailable, "Lyrics unavailable");
        assert_eq!(en.lyrics_hint, "Synchronized lyrics will appear during playback");
        assert_eq!(en.session_anonymous_hint, "Anonymous session: paste cookies to unlock library");
        assert_eq!(en.session_cookie_placeholder, "Paste session cookies...");
        assert_eq!(en.sign_in, "Sign in");
        assert_eq!(en.session_signed_in_hint, "Signed in: library unlocked");
        assert_eq!(en.sign_out, "Sign out");
        assert_eq!(en.library_signed_out_hint, "Sign in from Settings to load your library.");
        assert_eq!(en.open_settings_label, "Open settings");
        assert_eq!(en.section_playlists, "Playlists");
        assert_eq!(en.empty_playlists, "No playlists found.");
        assert_eq!(en.section_albums, "Albums");
        assert_eq!(en.empty_albums, "No albums found.");
        assert_eq!(en.empty_detail, "No details available.");
        assert_eq!(en.play_button, "Play");
        assert_eq!(en.shuffle_button, "Shuffle");
        assert_eq!(en.empty_songs, "No songs available.");
        assert_eq!(en.section_theme, "Theme");
        assert_eq!(en.theme_white, "White");
        assert_eq!(en.theme_black_oled, "Black OLED");
        assert_eq!(en.theme_mono, "Mono");
        assert_eq!(en.theme_custom, "Custom");
        assert_eq!(
            en.custom_theme_empty_hint,
            "Place theme JSON files in ~/.config/moggyt/themes/ or ~/.config/moggyt/custom-theme.json"
        );
        assert_eq!(en.section_playback, "Playback");
        assert_eq!(en.playback_speed, "Playback speed");
        assert_eq!(en.sleep_timer_label, "Sleep timer");
        assert_eq!(en.sleep_timer_off_label, "Off");
        assert_eq!(en.sleep_timer_15m_label, "15m");
        assert_eq!(en.sleep_timer_30m_label, "30m");
        assert_eq!(en.sleep_timer_45m_label, "45m");
        assert_eq!(en.sleep_timer_60m_label, "60m");
        assert_eq!(en.sleep_timer_end_of_track_label, "End of track");
        assert_eq!(en.audio_quality_label, "Audio quality");
        assert_eq!(en.audio_quality_hint_label, "Applies from the next track");
        assert_eq!(en.audio_quality_low_label, "Low (<=64 kbps)");
        assert_eq!(en.audio_quality_standard_label, "Standard (~128 kbps)");
        assert_eq!(en.audio_quality_high_label, "High (>=192 kbps)");
        assert_eq!(en.songs_only_label, "Prefer songs over music videos");
        assert_eq!(en.crossfade, "Crossfade");
        assert_eq!(en.crossfade_hint, "Smooth transition between tracks");
        assert_eq!(en.stream_cache, "Stream Cache Limit");
        assert_eq!(en.stream_cache_hint, "Disk space quota for offline stream cache");
        assert_eq!(en.section_integrations, "Integrations");
        assert_eq!(en.discord_presence_label, "Discord Rich Presence");
        assert_eq!(
            en.discord_presence_hint_label,
            "Shows the current track in your Discord status while Discord is running."
        );
        assert_eq!(en.spotify_label, "Spotify Web API");
        assert_eq!(en.spotify_client_id_placeholder, "Spotify Client ID");
        assert_eq!(en.spotify_client_secret_placeholder, "Spotify Client Secret");
        assert_eq!(en.spotify_save_label, "Save credentials");
        assert_eq!(en.spotify_clear_label, "Clear credentials");
        assert_eq!(en.section_session, "Session");
        assert_eq!(en.status_signed_in, "Status: Signed in");
        assert_eq!(en.status_anonymous, "Status: Anonymous");
        assert_eq!(en.section_about, "About");
        assert_eq!(en.app_version, "App version");
        assert_eq!(en.update_status, "Update status");
        assert_eq!(en.explore_loading, "Loading explore...");
        assert_eq!(en.explore_empty, "No sections found in Explore.");
        assert_eq!(en.section_player_bar, "Player bar");
        assert_eq!(en.player_bar_classic, "Classic");
        assert_eq!(en.player_bar_floating, "Floating");
        assert_eq!(en.player_bar_artwork, "Show artwork");
        assert_eq!(en.player_bar_like, "Show like button");
        assert_eq!(en.player_bar_shuffle, "Show shuffle button");
        assert_eq!(en.player_bar_repeat, "Show repeat button");
        assert_eq!(en.player_bar_skip, "Show skip 10s buttons");
        assert_eq!(en.player_bar_queue, "Show queue button");
        assert_eq!(en.player_bar_volume, "Show volume slider");
        assert_eq!(en.import_title_label, "Import from Spotify");
        assert_eq!(en.import_button_label, "Import");
        assert_eq!(en.import_tab_link_label, "From link");
        assert_eq!(en.import_tab_file_label, "From file");
        assert_eq!(en.import_link_placeholder, "Spotify playlist or album link");
        assert_eq!(en.import_file_placeholder, "CSV file path");
        assert_eq!(en.import_name_placeholder, "Playlist name (optional)");
        assert_eq!(en.import_to_queue_label, "Load directly to queue");
        assert_eq!(en.import_valid_link_label, "Valid Spotify link");
        assert_eq!(en.import_invalid_link_label, "Invalid Spotify link");
        assert_eq!(en.import_start_label, "Start import");
        assert_eq!(en.import_cancel_label, "Cancel");
        assert_eq!(en.import_progress_title, "Import progress");
        assert_eq!(en.import_matched_label, "Matched");
        assert_eq!(en.import_review_label, "Needs review");
        assert_eq!(en.import_unresolved_label, "Unresolved");
        assert_eq!(
            en.import_cap_hint,
            "Embed imports are capped at 100 tracks. Export a CSV for longer playlists."
        );
        assert_eq!(en.import_close_label, "Close");
        assert_eq!(en.import_review_title, "Review matches");
        assert_eq!(en.import_source_label, "Source");
        assert_eq!(en.import_matched_track_label, "Matched track");
        assert_eq!(en.import_high_confidence_label, "High confidence");
        assert_eq!(en.import_medium_confidence_label, "Medium confidence");
        assert_eq!(en.import_low_confidence_label, "Low confidence");
        assert_eq!(en.import_no_match_label, "No match");
        assert_eq!(en.import_swap_label, "Swap");
        assert_eq!(en.import_choose_track_label, "Choose a track");
        assert_eq!(en.import_save_label, "Save playlist");
        assert_eq!(en.import_discard_label, "Discard");
        assert_eq!(en.import_all_matched_label, "All matched");
        assert_eq!(en.import_custom_id_placeholder, "YouTube video ID");
        assert_eq!(en.import_use_id_label, "Use ID");
        assert_eq!(en.nav_stats, "Stats");
        assert_eq!(en.stats_last_4_weeks, "Last 4 weeks");
        assert_eq!(en.stats_last_6_months, "Last 6 months");
        assert_eq!(en.stats_all_time, "All time");
        assert_eq!(en.stats_plays, "Plays");
        assert_eq!(en.stats_listening_time, "Listening time");
        assert_eq!(en.stats_tracks, "Tracks");
        assert_eq!(en.stats_artists, "Artists");
        assert_eq!(en.stats_top_artists, "Top artists");
        assert_eq!(en.stats_top_tracks, "Top tracks");
        assert_eq!(
            en.stats_empty,
            "No plays recorded yet. Songs you listen to for at least 30 seconds show up here."
        );
    }
}
