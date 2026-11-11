//! Localized static user interface labels and window property bindings.
//!
//! Hexagonal invariant: pure in-memory label resolution and Slint window property dispatch.

use slint::ComponentHandle;

use crate::i18n::{Locale, tr};
use crate::slint_shell::{Labels, MoggytWindow};

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
    pub filter_lyrics: &'static str,
    pub filter_albums: &'static str,
    pub filter_artists: &'static str,
    pub filter_playlists: &'static str,
    pub search_empty: &'static str,
    pub search_loading: &'static str,
    pub search_error: &'static str,
    pub recent_searches_title: &'static str,
    pub recent_searches_empty: &'static str,
    pub clear_history: &'static str,
    pub queue_title: &'static str,
    pub queue_empty: &'static str,
    pub clear_queue_label: &'static str,
    pub no_cover: &'static str,
    pub lyrics_unavailable: &'static str,
    pub lyrics_hint: &'static str,
    pub lyrics_tab_label: &'static str,
    pub up_next_tab_label: &'static str,
    pub now_playing_cover_label: &'static str,
    pub now_playing_vinyl_label: &'static str,
    pub session_anonymous_hint: &'static str,
    pub session_cookie_placeholder: &'static str,
    pub sign_in: &'static str,
    pub session_signed_in_hint: &'static str,
    pub sign_out: &'static str,
    pub library_signed_out_hint: &'static str,
    pub open_settings_label: &'static str,
    pub section_playlists: &'static str,
    pub empty_playlists: &'static str,
    pub section_albums: &'static str,
    pub empty_albums: &'static str,
    pub empty_detail: &'static str,
    pub play_button: &'static str,
    pub shuffle_button: &'static str,
    pub empty_songs: &'static str,
    pub section_theme: &'static str,
    pub theme_white: &'static str,
    pub theme_black_oled: &'static str,
    pub theme_mono: &'static str,
    pub theme_custom: &'static str,
    pub custom_theme_empty_hint: &'static str,
    pub section_playback: &'static str,
    pub playback_speed: &'static str,
    pub sleep_timer_label: &'static str,
    pub sleep_timer_off_label: &'static str,
    pub sleep_timer_15m_label: &'static str,
    pub sleep_timer_30m_label: &'static str,
    pub sleep_timer_45m_label: &'static str,
    pub sleep_timer_60m_label: &'static str,
    pub sleep_timer_end_of_track_label: &'static str,
    pub audio_quality_label: &'static str,
    pub audio_quality_hint_label: &'static str,
    pub audio_quality_low_label: &'static str,
    pub audio_quality_standard_label: &'static str,
    pub audio_quality_high_label: &'static str,
    pub songs_only_label: &'static str,
    pub autoplay_label: &'static str,
    pub crossfade: &'static str,
    pub crossfade_hint: &'static str,
    pub stream_cache: &'static str,
    pub stream_cache_hint: &'static str,
    pub section_integrations: &'static str,
    pub discord_presence_label: &'static str,
    pub discord_presence_hint_label: &'static str,
    pub lastfm_label: &'static str,
    pub lastfm_connect_label: &'static str,
    pub lastfm_disconnect_label: &'static str,
    pub spotify_label: &'static str,
    pub spotify_client_id_placeholder: &'static str,
    pub spotify_client_secret_placeholder: &'static str,
    pub spotify_save_label: &'static str,
    pub spotify_clear_label: &'static str,
    pub section_session: &'static str,
    pub status_signed_in: &'static str,
    pub status_anonymous: &'static str,
    pub section_about: &'static str,
    pub app_version: &'static str,
    pub update_status: &'static str,
    pub explore_loading: &'static str,
    pub explore_empty: &'static str,
    pub section_player_bar: &'static str,
    pub player_bar_classic: &'static str,
    pub player_bar_floating: &'static str,
    pub player_bar_artwork: &'static str,
    pub player_bar_like: &'static str,
    pub player_bar_shuffle: &'static str,
    pub player_bar_repeat: &'static str,
    pub player_bar_skip: &'static str,
    pub player_bar_queue: &'static str,
    pub player_bar_volume: &'static str,
    pub import_title_label: &'static str,
    pub import_button_label: &'static str,
    pub import_tab_link_label: &'static str,
    pub import_tab_file_label: &'static str,
    pub import_link_placeholder: &'static str,
    pub import_file_placeholder: &'static str,
    pub import_name_placeholder: &'static str,
    pub import_to_queue_label: &'static str,
    pub import_valid_link_label: &'static str,
    pub import_invalid_link_label: &'static str,
    pub import_start_label: &'static str,
    pub import_cancel_label: &'static str,
    pub import_progress_title: &'static str,
    pub import_matched_label: &'static str,
    pub import_review_label: &'static str,
    pub import_unresolved_label: &'static str,
    pub import_cap_hint: &'static str,
    pub import_close_label: &'static str,
    pub import_review_title: &'static str,
    pub import_source_label: &'static str,
    pub import_matched_track_label: &'static str,
    pub import_high_confidence_label: &'static str,
    pub import_medium_confidence_label: &'static str,
    pub import_low_confidence_label: &'static str,
    pub import_no_match_label: &'static str,
    pub import_swap_label: &'static str,
    pub import_open_label: &'static str,
    pub import_choose_track_label: &'static str,
    pub import_save_label: &'static str,
    pub import_discard_label: &'static str,
    pub import_all_matched_label: &'static str,
    pub import_custom_id_placeholder: &'static str,
    pub import_use_id_label: &'static str,
    pub import_connection_lost_label: &'static str,
    pub import_retry_label: &'static str,
    pub nav_stats: &'static str,
    pub stats_last_4_weeks: &'static str,
    pub stats_last_6_months: &'static str,
    pub stats_all_time: &'static str,
    pub stats_plays: &'static str,
    pub stats_listening_time: &'static str,
    pub stats_tracks: &'static str,
    pub stats_artists: &'static str,
    pub stats_top_artists: &'static str,
    pub stats_top_tracks: &'static str,
    pub stats_empty: &'static str,
    pub local_rename_placeholder: &'static str,
    pub local_rename_label: &'static str,
    pub local_delete_label: &'static str,
    pub local_remove_track_label: &'static str,
    pub local_move_up_label: &'static str,
    pub local_move_down_label: &'static str,
    pub version_fix_open_label: &'static str,
    pub version_fix_title_label: &'static str,
    pub version_fix_search_label: &'static str,
    pub version_fix_empty_label: &'static str,
    pub version_fix_hint_label: &'static str,
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
            filter_lyrics: tr(locale, "Lyrics"),
            filter_albums: tr(locale, "Albums"),
            filter_artists: tr(locale, "Artists"),
            filter_playlists: tr(locale, "Playlists"),
            search_empty: tr(locale, "No results found."),
            search_loading: tr(locale, "Searching..."),
            search_error: tr(locale, "Failed to load search results"),
            recent_searches_title: tr(locale, "Recent searches"),
            recent_searches_empty: tr(locale, "No recent searches."),
            clear_history: tr(locale, "Clear history"),
            queue_title: tr(locale, "Queue"),
            queue_empty: tr(locale, "The queue is empty."),
            clear_queue_label: tr(locale, "Clear queue"),
            no_cover: tr(locale, "No cover"),
            lyrics_unavailable: tr(locale, "Lyrics unavailable"),
            lyrics_hint: tr(locale, "Synchronized lyrics will appear during playback"),
            lyrics_tab_label: tr(locale, "Lyrics"),
            up_next_tab_label: tr(locale, "Up next"),
            now_playing_cover_label: tr(locale, "Cover"),
            now_playing_vinyl_label: tr(locale, "Vinyl"),
            session_anonymous_hint: tr(
                locale,
                "Anonymous session: paste cookies to unlock library",
            ),
            session_cookie_placeholder: tr(locale, "Paste session cookies..."),
            sign_in: tr(locale, "Sign in"),
            session_signed_in_hint: tr(locale, "Signed in: library unlocked"),
            sign_out: tr(locale, "Sign out"),
            library_signed_out_hint: tr(locale, "Sign in from Settings to load your library."),
            open_settings_label: tr(locale, "Open settings"),
            section_playlists: tr(locale, "Playlists"),
            empty_playlists: tr(locale, "No playlists found."),
            section_albums: tr(locale, "Albums"),
            empty_albums: tr(locale, "No albums found."),
            empty_detail: tr(locale, "No details available."),
            play_button: tr(locale, "Play"),
            shuffle_button: tr(locale, "Shuffle"),
            empty_songs: tr(locale, "No songs available."),
            section_theme: tr(locale, "Theme"),
            theme_white: tr(locale, "White"),
            theme_black_oled: tr(locale, "Black OLED"),
            theme_mono: tr(locale, "Mono"),
            theme_custom: tr(locale, "Custom"),
            custom_theme_empty_hint: tr(
                locale,
                "Place theme JSON files in ~/.config/moggyt/themes/ or ~/.config/moggyt/custom-theme.json",
            ),
            section_playback: tr(locale, "Playback"),
            playback_speed: tr(locale, "Playback speed"),
            sleep_timer_label: tr(locale, "Sleep timer"),
            sleep_timer_off_label: tr(locale, "Off"),
            sleep_timer_15m_label: tr(locale, "15m"),
            sleep_timer_30m_label: tr(locale, "30m"),
            sleep_timer_45m_label: tr(locale, "45m"),
            sleep_timer_60m_label: tr(locale, "60m"),
            sleep_timer_end_of_track_label: tr(locale, "End of track"),
            audio_quality_label: tr(locale, "Audio quality"),
            audio_quality_hint_label: tr(locale, "Applies from the next track"),
            audio_quality_low_label: tr(locale, "Low (<=64 kbps)"),
            audio_quality_standard_label: tr(locale, "Standard (~128 kbps)"),
            audio_quality_high_label: tr(locale, "High (AAC max ~130 kbps)"),
            songs_only_label: tr(locale, "Prefer songs over music videos"),
            autoplay_label: tr(locale, "Autoplay similar tracks"),
            crossfade: tr(locale, "Crossfade"),
            crossfade_hint: tr(locale, "Smooth transition between tracks"),
            stream_cache: tr(locale, "Stream Cache Limit"),
            stream_cache_hint: tr(locale, "Disk space quota for offline stream cache"),
            section_integrations: tr(locale, "Integrations"),
            discord_presence_label: tr(locale, "Discord Rich Presence"),
            discord_presence_hint_label: tr(
                locale,
                "Shows the current track in your Discord status while Discord is running.",
            ),
            lastfm_label: tr(locale, "Last.fm"),
            lastfm_connect_label: tr(locale, "Connect with Last.fm"),
            lastfm_disconnect_label: tr(locale, "Disconnect"),
            spotify_label: tr(locale, "Spotify Web API"),
            spotify_client_id_placeholder: tr(locale, "Spotify Client ID"),
            spotify_client_secret_placeholder: tr(locale, "Spotify Client Secret"),
            spotify_save_label: tr(locale, "Save credentials"),
            spotify_clear_label: tr(locale, "Clear credentials"),
            section_session: tr(locale, "Session"),
            status_signed_in: tr(locale, "Status: Signed in"),
            status_anonymous: tr(locale, "Status: Anonymous"),
            section_about: tr(locale, "About"),
            app_version: tr(locale, "App version"),
            update_status: tr(locale, "Update status"),
            explore_loading: tr(locale, "Loading explore..."),
            explore_empty: tr(locale, "No sections found in Explore."),
            section_player_bar: tr(locale, "Player bar"),
            player_bar_classic: tr(locale, "Classic"),
            player_bar_floating: tr(locale, "Floating"),
            player_bar_artwork: tr(locale, "Show artwork"),
            player_bar_like: tr(locale, "Show like button"),
            player_bar_shuffle: tr(locale, "Show shuffle button"),
            player_bar_repeat: tr(locale, "Show repeat button"),
            player_bar_skip: tr(locale, "Show skip 10s buttons"),
            player_bar_queue: tr(locale, "Show queue button"),
            player_bar_volume: tr(locale, "Show volume slider"),
            import_title_label: tr(locale, "Import from Spotify"),
            import_button_label: tr(locale, "Import"),
            import_tab_link_label: tr(locale, "From link"),
            import_tab_file_label: tr(locale, "From file"),
            import_link_placeholder: tr(locale, "Spotify playlist or album link"),
            import_file_placeholder: tr(locale, "CSV file path"),
            import_name_placeholder: tr(locale, "Playlist name (optional)"),
            import_to_queue_label: tr(locale, "Load directly to queue"),
            import_valid_link_label: tr(locale, "Valid Spotify link"),
            import_invalid_link_label: tr(locale, "Invalid Spotify link"),
            import_start_label: tr(locale, "Start import"),
            import_cancel_label: tr(locale, "Cancel"),
            import_progress_title: tr(locale, "Import progress"),
            import_matched_label: tr(locale, "Matched"),
            import_review_label: tr(locale, "Needs review"),
            import_unresolved_label: tr(locale, "Unresolved"),
            import_cap_hint: tr(
                locale,
                "Embed imports are capped at 100 tracks. Export a CSV for longer playlists.",
            ),
            import_close_label: tr(locale, "Close"),
            import_review_title: tr(locale, "Review matches"),
            import_source_label: tr(locale, "Source"),
            import_matched_track_label: tr(locale, "Matched track"),
            import_high_confidence_label: tr(locale, "High confidence"),
            import_medium_confidence_label: tr(locale, "Medium confidence"),
            import_low_confidence_label: tr(locale, "Low confidence"),
            import_no_match_label: tr(locale, "No match"),
            import_swap_label: tr(locale, "Swap"),
            import_open_label: tr(locale, "Open"),
            import_choose_track_label: tr(locale, "Choose a track"),
            import_save_label: tr(locale, "Save playlist"),
            import_discard_label: tr(locale, "Discard"),
            import_all_matched_label: tr(locale, "All matched"),
            import_custom_id_placeholder: tr(locale, "YouTube video ID"),
            import_use_id_label: tr(locale, "Use ID"),
            import_connection_lost_label: tr(
                locale,
                "Connection lost. Retry or save the matched tracks.",
            ),
            import_retry_label: tr(locale, "Retry"),
            nav_stats: tr(locale, "Stats"),
            stats_last_4_weeks: tr(locale, "Last 4 weeks"),
            stats_last_6_months: tr(locale, "Last 6 months"),
            stats_all_time: tr(locale, "All time"),
            stats_plays: tr(locale, "Plays"),
            stats_listening_time: tr(locale, "Listening time"),
            stats_tracks: tr(locale, "Tracks"),
            stats_artists: tr(locale, "Artists"),
            stats_top_artists: tr(locale, "Top artists"),
            stats_top_tracks: tr(locale, "Top tracks"),
            stats_empty: tr(
                locale,
                "No plays recorded yet. Songs you listen to for at least 30 seconds show up here.",
            ),
            local_rename_placeholder: tr(locale, "New playlist name"),
            local_rename_label: tr(locale, "Rename"),
            local_delete_label: tr(locale, "Delete playlist"),
            local_remove_track_label: tr(locale, "Remove"),
            local_move_up_label: tr(locale, "Up"),
            local_move_down_label: tr(locale, "Down"),
            version_fix_open_label: tr(locale, "Fix version"),
            version_fix_title_label: tr(locale, "Fix audio version"),
            version_fix_search_label: tr(locale, "Search alternatives"),
            version_fix_empty_label: tr(locale, "No alternatives found"),
            version_fix_hint_label: tr(locale, "Search for a different recording"),
        }
    }

    /// Pushes all resolved static labels into Slint window properties and the global labels store.
    pub fn apply(&self, window: &MoggytWindow) {
        let labels = window.global::<Labels>();
        labels.set_nav_search_label(self.nav_search.into());
        labels.set_nav_explore_label(self.nav_explore.into());
        labels.set_nav_library_label(self.nav_library.into());
        labels.set_nav_now_playing_label(self.nav_now_playing.into());

        labels.set_search_placeholder(self.search_placeholder.into());
        labels.set_search_button_label(self.search_button.into());
        labels.set_filter_all_label(self.filter_all.into());
        labels.set_filter_songs_label(self.filter_songs.into());
        labels.set_filter_lyrics_label(self.filter_lyrics.into());
        labels.set_filter_albums_label(self.filter_albums.into());
        labels.set_filter_artists_label(self.filter_artists.into());
        labels.set_filter_playlists_label(self.filter_playlists.into());
        labels.set_search_empty_text(self.search_empty.into());
        labels.set_search_loading_text(self.search_loading.into());
        labels.set_search_error_text(self.search_error.into());
        labels.set_recent_title_label(self.recent_searches_title.into());
        labels.set_recent_empty_label(self.recent_searches_empty.into());
        labels.set_clear_history_label(self.clear_history.into());

        labels.set_queue_title_label(self.queue_title.into());
        labels.set_queue_empty_text(self.queue_empty.into());
        labels.set_clear_queue_label(self.clear_queue_label.into());

        labels.set_no_cover_text(self.no_cover.into());
        labels.set_lyrics_unavailable_text(self.lyrics_unavailable.into());
        labels.set_lyrics_hint_text(self.lyrics_hint.into());
        labels.set_lyrics_tab_label(self.lyrics_tab_label.into());
        labels.set_up_next_tab_label(self.up_next_tab_label.into());
        labels.set_now_playing_cover_label(self.now_playing_cover_label.into());
        labels.set_now_playing_vinyl_label(self.now_playing_vinyl_label.into());

        labels.set_session_anonymous_hint(self.session_anonymous_hint.into());
        labels.set_session_cookie_placeholder(self.session_cookie_placeholder.into());
        labels.set_sign_in_label(self.sign_in.into());
        labels.set_session_signed_in_hint(self.session_signed_in_hint.into());
        labels.set_sign_out_label(self.sign_out.into());
        labels.set_library_signed_out_hint(self.library_signed_out_hint.into());
        labels.set_open_settings_label(self.open_settings_label.into());
        labels.set_section_playlists_label(self.section_playlists.into());
        labels.set_empty_playlists_text(self.empty_playlists.into());
        labels.set_section_albums_label(self.section_albums.into());
        labels.set_empty_albums_text(self.empty_albums.into());

        labels.set_empty_detail_text(self.empty_detail.into());
        labels.set_play_button_label(self.play_button.into());
        labels.set_shuffle_button_label(self.shuffle_button.into());
        labels.set_empty_songs_text(self.empty_songs.into());

        labels.set_section_theme_label(self.section_theme.into());
        labels.set_theme_white_label(self.theme_white.into());
        labels.set_theme_black_oled_label(self.theme_black_oled.into());
        labels.set_theme_mono_label(self.theme_mono.into());
        labels.set_theme_custom_label(self.theme_custom.into());
        labels.set_custom_theme_empty_hint_label(self.custom_theme_empty_hint.into());
        labels.set_section_playback_label(self.section_playback.into());
        labels.set_playback_speed_label(self.playback_speed.into());
        labels.set_sleep_timer_label(self.sleep_timer_label.into());
        labels.set_sleep_timer_off_label(self.sleep_timer_off_label.into());
        labels.set_sleep_timer_15m_label(self.sleep_timer_15m_label.into());
        labels.set_sleep_timer_30m_label(self.sleep_timer_30m_label.into());
        labels.set_sleep_timer_45m_label(self.sleep_timer_45m_label.into());
        labels.set_sleep_timer_60m_label(self.sleep_timer_60m_label.into());
        labels.set_sleep_timer_end_of_track_label(self.sleep_timer_end_of_track_label.into());
        labels.set_audio_quality_label(self.audio_quality_label.into());
        labels.set_audio_quality_hint_label(self.audio_quality_hint_label.into());
        labels.set_audio_quality_low_label(self.audio_quality_low_label.into());
        labels.set_audio_quality_standard_label(self.audio_quality_standard_label.into());
        labels.set_audio_quality_high_label(self.audio_quality_high_label.into());
        labels.set_songs_only_label(self.songs_only_label.into());
        labels.set_autoplay_label(self.autoplay_label.into());
        labels.set_crossfade_label(self.crossfade.into());
        labels.set_crossfade_hint_label(self.crossfade_hint.into());
        labels.set_stream_cache_label(self.stream_cache.into());
        labels.set_stream_cache_hint_label(self.stream_cache_hint.into());
        labels.set_section_integrations_label(self.section_integrations.into());
        labels.set_discord_presence_label(self.discord_presence_label.into());
        labels.set_discord_presence_hint_label(self.discord_presence_hint_label.into());
        labels.set_lastfm_label(self.lastfm_label.into());
        labels.set_lastfm_connect_label(self.lastfm_connect_label.into());
        labels.set_lastfm_disconnect_label(self.lastfm_disconnect_label.into());
        labels.set_spotify_label(self.spotify_label.into());
        labels.set_spotify_client_id_placeholder(self.spotify_client_id_placeholder.into());
        labels.set_spotify_client_secret_placeholder(self.spotify_client_secret_placeholder.into());
        labels.set_spotify_save_label(self.spotify_save_label.into());
        labels.set_spotify_clear_label(self.spotify_clear_label.into());
        labels.set_section_session_label(self.section_session.into());
        labels.set_status_signed_in_label(self.status_signed_in.into());
        labels.set_status_anonymous_label(self.status_anonymous.into());
        labels.set_section_about_label(self.section_about.into());
        labels.set_app_version_label(self.app_version.into());
        labels.set_update_status_label(self.update_status.into());
        labels.set_explore_loading_text(self.explore_loading.into());
        labels.set_explore_empty_text(self.explore_empty.into());
        labels.set_section_player_bar_label(self.section_player_bar.into());
        labels.set_player_bar_classic_label(self.player_bar_classic.into());
        labels.set_player_bar_floating_label(self.player_bar_floating.into());
        labels.set_player_bar_artwork_label(self.player_bar_artwork.into());
        labels.set_player_bar_like_label(self.player_bar_like.into());
        labels.set_player_bar_shuffle_label(self.player_bar_shuffle.into());
        labels.set_player_bar_repeat_label(self.player_bar_repeat.into());
        labels.set_player_bar_skip_label(self.player_bar_skip.into());
        labels.set_player_bar_queue_label(self.player_bar_queue.into());
        labels.set_player_bar_volume_label(self.player_bar_volume.into());
        labels.set_import_title_label(self.import_title_label.into());
        labels.set_import_button_label(self.import_button_label.into());
        labels.set_import_tab_link_label(self.import_tab_link_label.into());
        labels.set_import_tab_file_label(self.import_tab_file_label.into());
        labels.set_import_link_placeholder(self.import_link_placeholder.into());
        labels.set_import_file_placeholder(self.import_file_placeholder.into());
        labels.set_import_name_placeholder(self.import_name_placeholder.into());
        labels.set_import_to_queue_label(self.import_to_queue_label.into());
        labels.set_import_valid_link_label(self.import_valid_link_label.into());
        labels.set_import_invalid_link_label(self.import_invalid_link_label.into());
        labels.set_import_start_label(self.import_start_label.into());
        labels.set_import_cancel_label(self.import_cancel_label.into());
        labels.set_import_progress_title(self.import_progress_title.into());
        labels.set_import_matched_label(self.import_matched_label.into());
        labels.set_import_review_label(self.import_review_label.into());
        labels.set_import_unresolved_label(self.import_unresolved_label.into());
        labels.set_import_cap_hint(self.import_cap_hint.into());
        labels.set_import_close_label(self.import_close_label.into());
        labels.set_import_review_title(self.import_review_title.into());
        labels.set_import_source_label(self.import_source_label.into());
        labels.set_import_matched_track_label(self.import_matched_track_label.into());
        labels.set_import_high_confidence_label(self.import_high_confidence_label.into());
        labels.set_import_medium_confidence_label(self.import_medium_confidence_label.into());
        labels.set_import_low_confidence_label(self.import_low_confidence_label.into());
        labels.set_import_no_match_label(self.import_no_match_label.into());
        labels.set_import_swap_label(self.import_swap_label.into());
        labels.set_import_open_label(self.import_open_label.into());
        labels.set_import_choose_track_label(self.import_choose_track_label.into());
        labels.set_import_save_label(self.import_save_label.into());
        labels.set_import_discard_label(self.import_discard_label.into());
        labels.set_import_all_matched_label(self.import_all_matched_label.into());
        labels.set_import_custom_id_placeholder(self.import_custom_id_placeholder.into());
        labels.set_import_use_id_label(self.import_use_id_label.into());
        labels.set_import_connection_lost_label(self.import_connection_lost_label.into());
        labels.set_import_retry_label(self.import_retry_label.into());
        labels.set_nav_stats_label(self.nav_stats.into());
        labels.set_stats_last_4_weeks_label(self.stats_last_4_weeks.into());
        labels.set_stats_last_6_months_label(self.stats_last_6_months.into());
        labels.set_stats_all_time_label(self.stats_all_time.into());
        labels.set_stats_plays_caption(self.stats_plays.into());
        labels.set_stats_listening_time_caption(self.stats_listening_time.into());
        labels.set_stats_tracks_caption(self.stats_tracks.into());
        labels.set_stats_artists_caption(self.stats_artists.into());
        labels.set_stats_top_artists_header(self.stats_top_artists.into());
        labels.set_stats_top_tracks_header(self.stats_top_tracks.into());
        labels.set_stats_empty_label(self.stats_empty.into());
        labels.set_local_rename_placeholder(self.local_rename_placeholder.into());
        labels.set_local_rename_label(self.local_rename_label.into());
        labels.set_local_delete_label(self.local_delete_label.into());
        labels.set_local_remove_track_label(self.local_remove_track_label.into());
        labels.set_local_move_up_label(self.local_move_up_label.into());
        labels.set_local_move_down_label(self.local_move_down_label.into());
        labels.set_version_fix_open_label(self.version_fix_open_label.into());
        labels.set_version_fix_title_label(self.version_fix_title_label.into());
        labels.set_version_fix_search_label(self.version_fix_search_label.into());
        labels.set_version_fix_empty_label(self.version_fix_empty_label.into());
        labels.set_version_fix_hint_label(self.version_fix_hint_label.into());
    }
}

/// Synchronizes static user interface labels into Slint window properties based on locale.
pub fn sync_static_labels(window: &MoggytWindow, locale: Locale) {
    StaticLabels::resolve(locale).apply(window);
}

#[cfg(test)]
#[path = "labels_tests.rs"]
mod tests;
