//! Window property and view-model synchronization from application state.
//!
//! Hexagonal invariant: pure in-memory projections pushing domain models into Slint UI properties.

use std::collections::HashMap;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{PlaybackFacts, SearchFilter, Track, is_local_playlist_id};
use crate::i18n::{Locale, tr};
use crate::infra::{GithubUpdater, StoreHealth};
use crate::slint_shell::audio_snapshot::sync_audio_snapshot;
use crate::slint_shell::covers::resolve_row_cover_image;
use crate::slint_shell::detail::{
    detail_has_artwork, detail_kind_label, detail_subtitle, resolve_detail_cover_url,
};
use crate::slint_shell::explore::{chunk_card_rows, explore_card_shelves, tile_color_for};
use crate::slint_shell::format::{
    format_queue_label, format_sleep_timer_status, format_update_status, timer_preset_to_index,
};
use crate::slint_shell::history::library_item_subtitle;
use crate::slint_shell::labels::sync_static_labels;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::playback::{
    is_track_liked, progress_fraction, push_time_labels, repeat_mode_to_int,
    resolve_display_duration,
};
use crate::slint_shell::player_bar::mode_to_index;
use crate::slint_shell::search::{SearchRow, format_duration};
use crate::slint_shell::state::{PersistenceError, PersistenceStore, SlintAppState};
use crate::slint_shell::theme::{apply_palette, theme_to_index};
use crate::slint_shell::view_phase::{ViewId, ViewPhase};
use crate::slint_shell::{
    DetailUi, ExploreCardData, LocalEditUi, LyricLineData, LyricsHighlight, MoggytWindow,
    PlaybackUi, PlayerUi, ShelfData, SleepTimerUi, TrackRowData,
};

pub(crate) use super::sync_phase::{observe_view_phases, sync_view_phase};

fn persistence_error_key(error: PersistenceError) -> Option<&'static str> {
    match (error.store, error.health) {
        (PersistenceStore::Settings, StoreHealth::Quarantined) => {
            Some("Settings file was corrupt and was quarantined")
        }
        (PersistenceStore::Settings, StoreHealth::Incompatible) => {
            Some("Settings file uses a newer schema")
        }
        (PersistenceStore::Queue, StoreHealth::Quarantined) => {
            Some("Queue file was corrupt and was quarantined")
        }
        (PersistenceStore::Queue, StoreHealth::Incompatible) => {
            Some("Queue file uses a newer schema")
        }
        _ => None,
    }
}

fn persistence_error_text(locale: Locale, error: Option<PersistenceError>) -> String {
    error.and_then(persistence_error_key).map(|key| tr(locale, key).to_string()).unwrap_or_default()
}

/// Synchronizes state with Slint window properties without performing blocking I/O.
pub(crate) fn sync_window(window: &MoggytWindow, state: &mut SlintAppState) {
    apply_palette(window, &state.active_palette());
    observe_view_phases(state);
    window.set_persistence_error_text(
        persistence_error_text(state.locale, state.persistence_error).into(),
    );
    window.set_persistence_dismiss_text(tr(state.locale, "Dismiss").into());
    window.set_active_theme_index(theme_to_index(state.theme));
    window.set_accent_customized(state.accent_override.is_some());
    window.set_custom_theme_status_text(state.custom_theme_status.clone().into());
    window.set_matugen_theme_status_text(state.matugen_theme_status.clone().into());
    let custom_names: Vec<slint::SharedString> = state
        .custom_theme
        .as_ref()
        .map(|ct| ct.available_themes().into_iter().map(slint::SharedString::from).collect())
        .unwrap_or_default();
    let active_custom =
        state.custom_theme.as_ref().and_then(|ct| ct.active_theme_name()).unwrap_or("");
    window.set_custom_theme_names(ModelRc::new(VecModel::from(custom_names)));
    window.set_active_custom_theme_name(active_custom.into());
    window.set_active_page(state.page.to_index());
    sync_header_title(window, state);
    window.set_queue_panel_open(state.queue_panel_open);
    let player = window.global::<PlayerUi>();
    sync_audio_snapshot(window, state);
    window.set_songs_only(state.songs_only);
    window.global::<DetailUi>().set_compact_tracks(state.detail_compact_tracks);
    window.global::<PlaybackUi>().set_autoplay_enabled(state.autoplay);
    window.set_crossfade_seconds(state.crossfade_seconds as i32);
    window.set_stream_cache_limit_mb(state.stream_cache_limit_mb as i32);
    window.set_cover_cache_limit_mb(state.cover_cache_limit_mb as i32);
    window.set_signed_in(state.signed_in);
    window.set_library_session_expired(state.library_phase == ViewPhase::Expired);
    window.set_app_version(GithubUpdater::current_version().into());
    sync_update_status(window, state);

    player.set_is_shuffle(state.is_shuffle);
    player.set_repeat_mode(repeat_mode_to_int(state.queue.repeat()));
    window.set_queue_count(state.queue.len() as i32);
    window.set_queue_button_text(format_queue_label(state.locale, state.queue.len()).into());
    window.set_player_bar_mode(mode_to_index(state.player_bar.mode));
    window.set_player_bar_show_artwork(state.player_bar.show_artwork);
    window.set_player_bar_show_like(state.player_bar.show_like);
    window.set_player_bar_show_shuffle(state.player_bar.show_shuffle);
    window.set_player_bar_show_repeat(state.player_bar.show_repeat);
    window.set_player_bar_show_skip_seconds(state.player_bar.show_skip_seconds);
    window.set_player_bar_show_queue(state.player_bar.show_queue);
    window.set_player_bar_show_volume(state.player_bar.show_volume);
    window.set_player_bar_show_timer(state.player_bar.show_timer);
    window.set_player_bar_show_speed(state.player_bar.show_speed);
    sync_static_labels(window, state.locale, state.custom_theme.as_ref().map(|c| c.config_dir()));
    let is_liked =
        state.queue.current().map(|t| is_track_liked(&state.liked_ids, &t.id)).unwrap_or(false);
    player.set_is_liked(is_liked);

    let (title, artist, catalog_dur) = match state.queue.current() {
        Some(t) => (t.title.clone(), t.artist.clone(), t.duration_secs),
        None => (tr(state.locale, "No track playing").to_string(), String::new(), None),
    };
    window.set_active_track_title(title.into());
    window.set_active_track_artist(artist.into());

    let (pos_secs, decoded_secs) = state
        .audio
        .try_lock()
        .map(|a| (a.position_secs() as u32, a.duration_secs()))
        .unwrap_or((0, 0));
    let dur = resolve_display_duration(catalog_dur, decoded_secs);
    push_time_labels(&player, pos_secs, dur);
    player.set_progress(progress_fraction(pos_secs, dur.unwrap_or(0)));

    sync_cover_and_lyrics(window, state);
    sync_recent_searches(window, state);
    sync_shelves_and_queue(window, state);
    sync_library_and_detail(window, state);
    sync_explore_shelves(window, state);
    sync_category_shelves(window, state);
    sync_view_phase(window, ViewId::Explore, state.explore_phase);
    sync_view_phase(window, ViewId::Category, state.category_phase);
    sync_view_phase(window, ViewId::Search, state.search_phase);
    sync_sleep_timer(window, state);
}

/// Synchronizes update availability and status text with Slint window.
pub(crate) fn sync_update_status(window: &MoggytWindow, state: &SlintAppState) {
    window.set_has_update(state.update_available);
    let update_status =
        format_update_status(state.locale, state.update_available, state.latest_version.as_deref());
    window.set_update_status_text(update_status.into());
}

/// Synchronizes cover art texture and synced lyrics lines with Slint window from pre-decoded state.
pub(crate) fn sync_cover_and_lyrics(window: &MoggytWindow, state: &mut SlintAppState) {
    if let Some(img) = &state.now_playing_cover {
        window.set_now_playing_cover(img.clone());
    } else if let Some(img) = state
        .queue
        .current()
        .and_then(|t| t.artwork.as_ref())
        .and_then(|u| state.search_covers.get(u))
    {
        state.now_playing_cover = Some(img.clone());
        window.set_now_playing_cover(img.clone());
    } else {
        window.set_now_playing_cover(slint::Image::default());
    }

    if let Some(img) = &state.current_cover_image {
        window.set_cover_image(img.clone());
        window.set_has_cover(true);
    } else {
        window.set_has_cover(false);
    }
    let line_data: Vec<LyricLineData> = state
        .lyrics_lines
        .iter()
        .enumerate()
        .map(|(i, l)| LyricLineData { index: i as i32, text: l.text.clone().into() })
        .collect();
    window.set_lyrics_lines(ModelRc::new(VecModel::from(line_data)));
    window.set_active_lyric_index(state.active_lyric_index());
}

/// Synchronizes catalog shelves and playback queue rows with Slint window.
pub(crate) fn sync_shelves_and_queue(window: &MoggytWindow, state: &SlintAppState) {
    let mut shelf_models = Vec::new();
    let mut global_idx = 0;
    for shelf in &state.home_shelves {
        let mut tracks = Vec::new();
        for item in &shelf.items {
            if state.search_filter != SearchFilter::All && item.kind() != state.search_filter {
                continue;
            }
            let row = SearchRow::from(item);
            let cover_img =
                resolve_row_cover_image(row.artwork_url.as_deref(), &state.search_covers);
            tracks.push(TrackRowData {
                index: global_idx,
                title: row.title.into(),
                artist: row.subtitle.into(),
                album: row.album.into(),
                duration: row.duration.into(),
                kind: row.kind.into(),
                cover: cover_img,
            });
            global_idx += 1;
        }
        shelf_models.push(ShelfData {
            title: shelf.title.clone().into(),
            tracks: ModelRc::new(VecModel::from(tracks)),
        });
    }
    window.set_shelves(ModelRc::new(VecModel::from(shelf_models)));

    let q_tracks: Vec<TrackRowData> = state
        .queue
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, t)| track_to_row_data(i, t, &state.search_covers))
        .collect();
    window.set_queue_tracks(ModelRc::new(VecModel::from(q_tracks)));
    window.set_queue_selected_index(state.queue.cursor() as i32);
    super::search_top::sync_top_result(window, state);
}

/// Synchronizes recent search queries and search-active state with Slint window.
pub(crate) fn sync_recent_searches(window: &MoggytWindow, state: &SlintAppState) {
    let rows: Vec<slint::SharedString> = state
        .recent_searches
        .entries()
        .iter()
        .map(|s| slint::SharedString::from(s.as_str()))
        .collect();
    window.set_recent_searches(ModelRc::new(VecModel::from(rows)));
    window.set_search_active(state.search_active);
}

/// Persists current recent searches to the settings store, ignoring save errors.
pub(crate) fn persist_recent_searches(state: &SlintAppState) {
    let mut settings = state.settings_store.load();
    settings.recent_searches = state.recent_searches.clone();
    let _ = state.settings_store.save(&settings);
}

fn playlist_cards(state: &SlintAppState) -> Vec<ExploreCardData> {
    state
        .library_playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| ExploreCardData {
            index: index as i32,
            title: playlist.title.clone().into(),
            subtitle: library_item_subtitle(playlist, state.locale).into(),
            has_cover: playlist.artwork.is_some(),
            cover: resolve_row_cover_image(playlist.artwork.as_deref(), &state.search_covers),
            tile_color: tile_color_for(&playlist.title),
        })
        .collect()
}

fn album_cards(state: &SlintAppState) -> Vec<ExploreCardData> {
    state
        .library_albums
        .iter()
        .enumerate()
        .map(|(index, album)| ExploreCardData {
            index: index as i32,
            title: album.title.clone().into(),
            subtitle: album.caption().into(),
            has_cover: album.artwork.is_some(),
            cover: resolve_row_cover_image(album.artwork.as_deref(), &state.search_covers),
            tile_color: tile_color_for(&album.title),
        })
        .collect()
}

fn artist_cards(state: &SlintAppState) -> Vec<ExploreCardData> {
    state
        .library_artists
        .iter()
        .enumerate()
        .map(|(index, artist)| ExploreCardData {
            index: index as i32,
            title: artist.name.clone().into(),
            subtitle: artist.subscribers.clone().unwrap_or_default().into(),
            has_cover: artist.artwork.is_some(),
            cover: resolve_row_cover_image(artist.artwork.as_deref(), &state.search_covers),
            tile_color: tile_color_for(&artist.name),
        })
        .collect()
}

fn profile_cards(state: &SlintAppState) -> Vec<ExploreCardData> {
    state
        .library_profiles
        .iter()
        .enumerate()
        .map(|(index, profile)| ExploreCardData {
            index: index as i32,
            title: profile.name.clone().into(),
            subtitle: profile.subscribers.clone().unwrap_or_default().into(),
            has_cover: profile.artwork.is_some(),
            cover: resolve_row_cover_image(profile.artwork.as_deref(), &state.search_covers),
            tile_color: tile_color_for(&profile.name),
        })
        .collect()
}

fn podcast_cards(state: &SlintAppState) -> Vec<ExploreCardData> {
    state
        .library_podcasts
        .iter()
        .enumerate()
        .map(|(index, podcast)| ExploreCardData {
            index: index as i32,
            title: podcast.title.clone().into(),
            subtitle: "".into(),
            has_cover: podcast.artwork.is_some(),
            cover: resolve_row_cover_image(podcast.artwork.as_deref(), &state.search_covers),
            tile_color: tile_color_for(&podcast.title),
        })
        .collect()
}

fn library_song_rows(state: &SlintAppState) -> Vec<TrackRowData> {
    state
        .library_songs
        .iter()
        .enumerate()
        .map(|(index, track)| track_to_row_data(index, track, &state.search_covers))
        .collect()
}

/// Synchronizes library collections and detail view models with Slint window.
pub(crate) fn sync_library_and_detail(window: &MoggytWindow, state: &SlintAppState) {
    window.set_library_session_expired(state.library_phase == ViewPhase::Expired);
    let playlist_rows = chunk_card_rows(playlist_cards(state), state.explore_columns);
    window.set_library_playlist_rows(ModelRc::new(VecModel::from(playlist_rows)));

    let album_rows = chunk_card_rows(album_cards(state), state.explore_columns);
    window.set_library_album_rows(ModelRc::new(VecModel::from(album_rows)));

    let artist_rows = chunk_card_rows(artist_cards(state), state.explore_columns);
    window.set_library_artist_rows(ModelRc::new(VecModel::from(artist_rows)));

    let podcast_rows = chunk_card_rows(podcast_cards(state), state.explore_columns);
    window.set_library_podcast_rows(ModelRc::new(VecModel::from(podcast_rows)));

    let profile_rows = chunk_card_rows(profile_cards(state), state.explore_columns);
    window.set_library_profile_rows(ModelRc::new(VecModel::from(profile_rows)));

    window.set_library_song_rows(ModelRc::new(VecModel::from(library_song_rows(state))));

    window.set_library_filter(state.library_filter.to_index());

    if let Some(detail) = &state.detail {
        window.set_detail_has_detail(true);
        window.set_detail_has_artwork(detail_has_artwork(detail));
        window.set_detail_title(detail.title.clone().into());
        window.set_detail_subtitle(detail_subtitle(detail, state.locale).into());
        window.set_detail_description(detail.description.as_deref().unwrap_or("").into());
        window.set_detail_kind_label(tr(state.locale, detail_kind_label(detail)).into());
        let d_tracks: Vec<TrackRowData> = detail
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| track_to_row_data(i, t, &state.search_covers))
            .collect();
        window.set_detail_tracks(ModelRc::new(VecModel::from(d_tracks)));
        let cover_img = resolve_detail_cover_url(detail)
            .map(|url| resolve_row_cover_image(Some(url), &state.search_covers))
            .unwrap_or_default();
        window.set_detail_cover(cover_img);
    } else {
        window.set_detail_has_detail(false);
        window.set_detail_has_artwork(false);
        window.set_detail_description("".into());
        window.set_detail_cover(slint::Image::default());
    }
    sync_local_edit_ui(window, state);
    super::detail_sections::sync_detail_extras(window, state);
}

fn sync_local_edit_ui(window: &MoggytWindow, state: &SlintAppState) {
    let local_ui = window.global::<LocalEditUi>();
    let is_local =
        state.detail_request.as_ref().map(|(id, _)| is_local_playlist_id(id)).unwrap_or(false);
    local_ui.set_is_local(is_local);
    if is_local {
        let title = state.detail.as_ref().map(|d| d.title.as_str()).unwrap_or("");
        local_ui.set_rename_text(title.into());
    }
}

/// Columns used until the window reports its real width.
pub(crate) const DEFAULT_EXPLORE_COLUMNS: usize = 5;

/// Synchronizes explore shelves with Slint window models.
pub(crate) fn sync_explore_shelves(window: &MoggytWindow, state: &SlintAppState) {
    window.set_explore_shelves(ModelRc::new(VecModel::from(explore_card_shelves(
        &state.explore_shelves,
        &state.search_covers,
        state.explore_columns,
    ))));
}

/// Synchronizes category shelves with Slint window models.
pub(crate) fn sync_category_shelves(window: &MoggytWindow, state: &SlintAppState) {
    window.set_category_shelves(ModelRc::new(VecModel::from(explore_card_shelves(
        &state.category_shelves,
        &state.search_covers,
        state.explore_columns,
    ))));
}

/// Resolves header title for active page, selecting custom title for Detail and Category views.
pub(crate) fn resolve_header_title(state: &SlintAppState) -> slint::SharedString {
    match state.page {
        SlintPage::Category => {
            let t = state.category_title.trim();
            if t.is_empty() { tr(state.locale, "Category").into() } else { t.into() }
        }
        SlintPage::Detail => state
            .detail
            .as_ref()
            .map(|d| d.title.trim())
            .filter(|t| !t.is_empty())
            .map_or_else(|| tr(state.locale, "Detail").into(), Into::into),
        page => tr(state.locale, page.title()).into(),
    }
}

/// Pushes the resolved header title for the active page into the window.
pub(crate) fn sync_header_title(window: &MoggytWindow, state: &SlintAppState) {
    window.set_header_title(resolve_header_title(state));
}

/// Pushes the sung/rest split of the active lyric line into the UI global.
pub(crate) fn set_lyrics_highlight(window: &MoggytWindow, sung: String, rest: String) {
    let highlight = window.global::<LyricsHighlight>();
    highlight.set_sung_text(sung.into());
    highlight.set_rest_text(rest.into());
}

/// Periodic playback tick syncing elapsed timestamps, progress, and lyrics line without blocking.
pub(crate) fn sync_playback_tick(window: &MoggytWindow, state: &mut SlintAppState) {
    state.poll_output_device();
    let Ok(audio) = state.audio.try_lock() else {
        return;
    };
    let pos_secs = audio.position_secs() as u32;
    let decoded_secs = audio.duration_secs();
    let is_playing = audio.is_playing();
    drop(audio);

    let catalog_dur = state.queue.current().and_then(|t| t.duration_secs);
    let dur = resolve_display_duration(catalog_dur, decoded_secs);
    let player = window.global::<PlayerUi>();
    push_time_labels(&player, pos_secs, dur);
    player.set_progress(progress_fraction(pos_secs, dur.unwrap_or(0)));
    window.set_active_lyric_index(state.active_lyric_index_at(pos_secs));
    let (sung, rest) = state.active_lyric_text_split_at(u64::from(pos_secs) * 1000);
    set_lyrics_highlight(window, sung, rest);
    player.set_is_playing(is_playing);
    sync_sleep_timer(window, state);
    sync_audio_snapshot(window, state);
}

/// Samples raw backend facts and advances the stored playback phase.
pub(crate) fn observe_playback_phase(state: &mut SlintAppState) {
    let duration_secs = state.queue.current().and_then(|t| t.duration_secs).map_or(0, u64::from);
    let Ok(audio) = state.audio.try_lock() else {
        return;
    };
    let facts = PlaybackFacts {
        has_track: audio.last_requested().is_some(),
        is_empty: audio.is_empty(),
        is_paused: audio.is_paused(),
        is_playing: audio.is_playing(),
        position_secs: audio.position_secs(),
        duration_secs: audio.duration_secs().max(duration_secs),
    };
    drop(audio);
    let Some(event) = facts.event() else {
        return;
    };
    state.set_playback_phase(state.playback_phase.next(event));
}

/// Synchronizes sleep timer preset selection index and formatted status text with the timer menu.
pub(crate) fn sync_sleep_timer(window: &MoggytWindow, state: &SlintAppState) {
    let preset = state.sleep_timer.preset();
    let idx = timer_preset_to_index(preset);
    let status =
        format_sleep_timer_status(state.locale, preset, state.sleep_timer.remaining_secs());
    let ui = window.global::<SleepTimerUi>();
    ui.set_active_index(idx);
    ui.set_status_text(status.into());
}

/// Converts a domain track to Slint TrackRowData with cached cover art if available.
pub(crate) fn track_to_row_data(
    index: usize,
    track: &Track,
    covers: &HashMap<String, slint::Image>,
) -> TrackRowData {
    let cover = resolve_row_cover_image(track.artwork.as_deref(), covers);
    TrackRowData {
        index: index as i32,
        title: track.title.clone().into(),
        artist: track.artist.clone().into(),
        album: track.album.clone().unwrap_or_default().into(),
        duration: format_duration(track.duration_secs).into(),
        kind: "track".into(),
        cover,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_to_row_data_uses_thumbnail_variant() {
        let track = Track::new("t1", "Title", "Artist")
            .with_artwork("https://lh3.googleusercontent.com/test-thumb-large");
        let covers = HashMap::new();

        let row_data = track_to_row_data(0, &track, &covers);
        assert_eq!(row_data.index, 0);
        assert_eq!(row_data.title.as_str(), "Title");
        assert_eq!(row_data.artist.as_str(), "Artist");
        assert_eq!(row_data.kind.as_str(), "track");
    }

    #[test]
    fn test_resolve_header_title_detail_and_category() {
        use crate::domain::{Detail, DetailKind};
        use crate::i18n::Locale;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        assert_eq!(resolve_header_title(&state).as_str(), "Search");

        state.page = SlintPage::Category;
        state.category_title = "Calma".to_owned();
        assert_eq!(resolve_header_title(&state).as_str(), "Calma");
        state.category_title.clear();
        assert_eq!(resolve_header_title(&state).as_str(), "Category");
        state.locale = Locale::Spanish;
        assert_eq!(resolve_header_title(&state).as_str(), "Categoria");

        state.page = SlintPage::Detail;
        state.detail = Some(Detail::new(DetailKind::Album, "Hits", "Artist", Vec::new()));
        assert_eq!(resolve_header_title(&state).as_str(), "Hits");
        state.detail = Some(Detail::new(DetailKind::Album, "   ", "Artist", Vec::new()));
        assert_eq!(resolve_header_title(&state).as_str(), "Detalle");
        state.detail = None;
        assert_eq!(resolve_header_title(&state).as_str(), "Detalle");
    }

    #[test]
    fn test_resolve_header_title_replaces_detail_loading_placeholder() {
        use crate::domain::{Detail, DetailKind};
        use crate::i18n::Locale;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.locale = Locale::Spanish;
        state.page = SlintPage::Detail;
        state.detail =
            Some(Detail::new(DetailKind::Album, tr(Locale::Spanish, "Loading..."), "", Vec::new()));
        assert_eq!(resolve_header_title(&state).as_str(), "Cargando...");

        state.detail = Some(Detail::new(DetailKind::Album, "Okupas", "Diel Paris", Vec::new()));
        assert_eq!(resolve_header_title(&state).as_str(), "Okupas");
    }

    #[test]
    fn observe_keeps_empty_phase_without_tracks() {
        use crate::domain::PlaybackPhase;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        observe_playback_phase(&mut state);
        assert_eq!(state.playback_phase, PlaybackPhase::Empty);
    }

    #[test]
    fn observe_marks_paused_after_pause() {
        use crate::domain::PlaybackPhase;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.queue.push(Track::new("t", "Title", "Artist").with_duration(200));
        state.set_playback_phase(PlaybackPhase::Playing);
        {
            let mut audio = state.audio.lock().unwrap();
            let _ = audio.play("t");
            audio.seek(10);
            audio.pause();
        }
        observe_playback_phase(&mut state);
        assert_eq!(state.playback_phase, PlaybackPhase::Paused);
    }

    #[test]
    fn observe_marks_playing_while_audible() {
        use crate::domain::PlaybackPhase;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.queue.push(Track::new("t", "Title", "Artist").with_duration(200));
        state.set_playback_phase(PlaybackPhase::Resolving);
        {
            let mut audio = state.audio.lock().unwrap();
            let _ = audio.play("t");
            audio.seek(10);
        }
        observe_playback_phase(&mut state);
        assert_eq!(state.playback_phase, PlaybackPhase::Playing);
    }

    #[test]
    fn test_persistence_error_text_mapping() {
        let err_settings_q = Some(PersistenceError {
            store: PersistenceStore::Settings,
            health: StoreHealth::Quarantined,
        });
        assert_eq!(
            persistence_error_text(Locale::English, err_settings_q),
            "Settings file was corrupt and was quarantined"
        );
        assert_eq!(
            persistence_error_text(Locale::Spanish, err_settings_q),
            "El archivo de ajustes estaba corrupto y se puso en cuarentena"
        );

        let err_settings_i = Some(PersistenceError {
            store: PersistenceStore::Settings,
            health: StoreHealth::Incompatible,
        });
        assert_eq!(
            persistence_error_text(Locale::English, err_settings_i),
            "Settings file uses a newer schema"
        );

        let err_queue_q = Some(PersistenceError {
            store: PersistenceStore::Queue,
            health: StoreHealth::Quarantined,
        });
        assert_eq!(
            persistence_error_text(Locale::English, err_queue_q),
            "Queue file was corrupt and was quarantined"
        );

        let err_queue_i = Some(PersistenceError {
            store: PersistenceStore::Queue,
            health: StoreHealth::Incompatible,
        });
        assert_eq!(
            persistence_error_text(Locale::English, err_queue_i),
            "Queue file uses a newer schema"
        );

        assert_eq!(persistence_error_text(Locale::English, None), "");
    }
}
