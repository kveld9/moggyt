//! Window property and view-model synchronization from application state.
//!
//! Hexagonal invariant: pure in-memory projections pushing domain models into Slint UI properties.

use std::collections::HashMap;

use slint::{ModelRc, VecModel};

use crate::domain::{SearchFilter, Track};
use crate::i18n::tr;
use crate::infra::GithubUpdater;
use crate::slint_shell::covers::resolve_row_cover_image;
use crate::slint_shell::detail::{detail_has_artwork, detail_kind_label, resolve_detail_cover_url};
use crate::slint_shell::explore::explore_card_shelves;
use crate::slint_shell::format::{
    format_playback_speed, format_queue_label, format_sleep_timer_status, format_update_status,
    timer_preset_to_index,
};
use crate::slint_shell::labels::sync_static_labels;
use crate::slint_shell::playback::{
    format_position, format_track_duration, is_track_liked, progress_fraction, repeat_mode_to_int,
};
use crate::slint_shell::player_bar::mode_to_index;
use crate::slint_shell::search::{SearchRow, format_duration};
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::theme::{apply_theme, theme_to_index};
use crate::slint_shell::{LibraryItemData, LyricLineData, MoggytWindow, ShelfData, TrackRowData};

/// Synchronizes state with Slint window properties without performing blocking I/O.
pub(crate) fn sync_window(window: &MoggytWindow, state: &mut SlintAppState) {
    apply_theme(window, state.theme);
    window.set_active_theme_index(theme_to_index(state.theme));
    window.set_active_page(state.page.to_index());
    window.set_header_title(tr(state.locale, state.page.title()).into());
    window.set_queue_panel_open(state.queue_panel_open);
    let is_playing = state.audio.try_lock().map(|a| a.is_playing()).unwrap_or(false);
    window.set_is_playing(is_playing);
    let volume = state.audio.try_lock().map(|a| a.volume()).unwrap_or(100);
    window.set_volume(volume as i32);
    let speed = state.audio.try_lock().map(|a| a.speed()).unwrap_or(1.0);
    window.set_playback_speed_text(format_playback_speed(speed).into());
    window.set_signed_in(state.signed_in);
    window.set_app_version(GithubUpdater::current_version().into());
    sync_update_status(window, state);

    window.set_is_shuffle(state.is_shuffle);
    window.set_repeat_mode(repeat_mode_to_int(state.queue.repeat()));
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
    sync_static_labels(window, state.locale);
    let is_liked =
        state.queue.current().map(|t| is_track_liked(&state.liked_ids, &t.id)).unwrap_or(false);
    window.set_is_liked(is_liked);

    let (title, artist, dur_str, dur_secs) = match state.queue.current() {
        Some(t) => (
            t.title.clone(),
            t.artist.clone(),
            format_track_duration(t.duration_secs),
            t.duration_secs.unwrap_or(0),
        ),
        None => {
            (tr(state.locale, "No track playing").to_string(), String::new(), "0:00".to_string(), 0)
        }
    };
    window.set_active_track_title(title.into());
    window.set_active_track_artist(artist.into());
    window.set_duration_text(dur_str.into());
    window.set_duration_secs(dur_secs as i32);

    let pos_secs = state.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0) as u32;
    window.set_position_secs(pos_secs as i32);
    window.set_position_text(format_position(pos_secs).into());
    window.set_playback_progress(progress_fraction(pos_secs, dur_secs));

    sync_cover_and_lyrics(window, state);
    sync_recent_searches(window, state);
    sync_shelves_and_queue(window, state);
    sync_library_and_detail(window, state);
    sync_explore_shelves(window, state);
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

/// Synchronizes library collections and detail view models with Slint window.
pub(crate) fn sync_library_and_detail(window: &MoggytWindow, state: &SlintAppState) {
    let playlists: Vec<LibraryItemData> = state
        .library_playlists
        .iter()
        .enumerate()
        .map(|(i, p)| LibraryItemData {
            index: i as i32,
            title: p.title.clone().into(),
            subtitle: format!("{} tracks", p.track_count).into(),
            cover: resolve_row_cover_image(p.artwork.as_deref(), &state.search_covers),
        })
        .collect();
    window.set_library_playlists(ModelRc::new(VecModel::from(playlists)));

    let albums: Vec<LibraryItemData> = state
        .library_albums
        .iter()
        .enumerate()
        .map(|(i, a)| LibraryItemData {
            index: i as i32,
            title: a.title.clone().into(),
            subtitle: a.artist.clone().into(),
            cover: resolve_row_cover_image(a.artwork.as_deref(), &state.search_covers),
        })
        .collect();
    window.set_library_albums(ModelRc::new(VecModel::from(albums)));

    if let Some(detail) = &state.detail {
        window.set_detail_has_detail(true);
        window.set_detail_has_artwork(detail_has_artwork(detail));
        window.set_detail_title(detail.title.clone().into());
        window.set_detail_subtitle(detail.subtitle.clone().into());
        window.set_detail_description(detail.description.as_deref().unwrap_or("").into());
        window.set_detail_kind_label(tr(state.locale, detail_kind_label(detail.kind)).into());
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

/// Periodic playback tick syncing elapsed timestamps, progress, and lyrics line without blocking.
pub(crate) fn sync_playback_tick(window: &MoggytWindow, state: &mut SlintAppState) {
    let Ok(audio) = state.audio.try_lock() else {
        return;
    };
    let pos_secs = audio.position_secs() as u32;
    let is_playing = audio.is_playing();
    drop(audio);

    let dur_secs = state.queue.current().and_then(|t| t.duration_secs).unwrap_or(0);
    window.set_position_secs(pos_secs as i32);
    window.set_duration_secs(dur_secs as i32);
    window.set_position_text(format_position(pos_secs).into());
    window.set_playback_progress(progress_fraction(pos_secs, dur_secs));
    window.set_active_lyric_index(state.active_lyric_index_at(pos_secs));
    window.set_is_playing(is_playing);
    sync_sleep_timer(window, state);
}

/// Synchronizes sleep timer preset selection index and formatted status text with Slint window.
pub(crate) fn sync_sleep_timer(window: &MoggytWindow, state: &SlintAppState) {
    let preset = state.sleep_timer.preset();
    let idx = timer_preset_to_index(preset);
    let status =
        format_sleep_timer_status(state.locale, preset, state.sleep_timer.remaining_secs());
    window.set_active_sleep_timer_index(idx);
    window.set_sleep_timer_status_text(status.into());
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
}
