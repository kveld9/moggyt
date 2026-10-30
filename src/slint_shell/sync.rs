//! Window property and view-model synchronization from application state.
//!
//! Hexagonal invariant: pure in-memory projections pushing domain models into Slint UI properties.

use std::collections::HashMap;

use slint::{ModelRc, VecModel};

use crate::domain::{SearchFilter, Shelf};
use crate::i18n::tr;
use crate::infra::{AudioBackend, Catalog, CoverCache, GithubUpdater, LyricsSource, SessionStore};
use crate::slint_shell::covers::{resolve_row_cover_image, track_to_row_data};
use crate::slint_shell::detail::{detail_has_artwork, detail_kind_label, resolve_detail_cover_url};
use crate::slint_shell::format::{format_playback_speed, format_queue_label, format_update_status};
use crate::slint_shell::labels::sync_static_labels;
use crate::slint_shell::playback::{
    format_position, format_track_duration, is_track_liked, progress_fraction, repeat_mode_to_int,
};
use crate::slint_shell::player_bar::mode_to_index;
use crate::slint_shell::search::SearchRow;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::theme::{apply_theme, theme_to_index};
use crate::slint_shell::{
    ExploreCardData, ExploreCardRow, ExploreShelfData, LibraryItemData, LyricLineData,
    MoggytWindow, ShelfData, TrackRowData,
};

/// Synchronizes state with Slint window properties without performing blocking I/O.
pub(crate) fn sync_window<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    apply_theme(window, state.theme);
    window.set_active_theme_index(theme_to_index(state.theme));
    window.set_active_page(state.page.to_index());
    window.set_header_title(tr(state.locale, state.page.title()).into());
    let is_playing = state.audio.try_lock().map(|a| a.is_playing()).unwrap_or(false);
    window.set_is_playing(is_playing);
    let volume = state.audio.try_lock().map(|a| a.volume()).unwrap_or(100);
    window.set_volume(volume as i32);
    let speed = state.audio.try_lock().map(|a| a.speed()).unwrap_or(1.0);
    window.set_playback_speed_text(format_playback_speed(speed).into());
    window.set_signed_in(state.signed_in);
    window.set_has_update(state.update_available);
    window.set_app_version(GithubUpdater::current_version().into());
    let update_status =
        format_update_status(state.locale, state.update_available, state.latest_version.as_deref());
    window.set_update_status_text(update_status.into());

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
    sync_shelves_and_queue(window, state);
    sync_library_and_detail(window, state);
    sync_explore_shelves(window, state);
}

/// Synchronizes cover art texture and synced lyrics lines with Slint window from pre-decoded state.
pub(crate) fn sync_cover_and_lyrics<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
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
pub(crate) fn sync_shelves_and_queue<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    window: &MoggytWindow,
    state: &SlintAppState<C, A, S, L, K>,
) {
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

/// Synchronizes library collections and detail view models with Slint window.
pub(crate) fn sync_library_and_detail<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    window: &MoggytWindow,
    state: &SlintAppState<C, A, S, L, K>,
) {
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

    let artists: Vec<LibraryItemData> = state
        .library_artists
        .iter()
        .enumerate()
        .map(|(i, a)| LibraryItemData {
            index: i as i32,
            title: a.name.clone().into(),
            subtitle: "Artist".into(),
            cover: resolve_row_cover_image(a.artwork.as_deref(), &state.search_covers),
        })
        .collect();
    window.set_library_artists(ModelRc::new(VecModel::from(artists)));

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

/// Card width in logical pixels; must match `width` in `ui/components/cards.slint`.
pub(crate) const EXPLORE_CARD_WIDTH: i32 = 150;
/// Horizontal gap between cards in logical pixels.
pub(crate) const EXPLORE_CARD_GAP: i32 = 12;
/// Sidebar width plus content padding in logical pixels.
pub(crate) const EXPLORE_CHROME_WIDTH: i32 = 248;
/// Column bounds: never fewer than 2, never more than 12.
const MIN_EXPLORE_COLUMNS: usize = 2;
const MAX_EXPLORE_COLUMNS: usize = 12;
/// Columns used until the window reports its real width.
pub(crate) const DEFAULT_EXPLORE_COLUMNS: usize = 5;

/// Number of cards that fit in one row for a given window width.
pub(crate) fn explore_columns_for_width(window_width: i32) -> usize {
    let usable = window_width - EXPLORE_CHROME_WIDTH + EXPLORE_CARD_GAP;
    let per_card = EXPLORE_CARD_WIDTH + EXPLORE_CARD_GAP;
    if usable <= 0 {
        return MIN_EXPLORE_COLUMNS;
    }
    ((usable / per_card) as usize).clamp(MIN_EXPLORE_COLUMNS, MAX_EXPLORE_COLUMNS)
}

/// Muted tile palette used for explore items that carry no artwork.
const TILE_PALETTE: [[u8; 3]; 8] = [
    [123, 77, 161],
    [45, 110, 142],
    [46, 125, 91],
    [138, 90, 43],
    [142, 59, 77],
    [63, 90, 142],
    [90, 107, 46],
    [107, 63, 122],
];

/// Picks a stable tile colour from the palette for a title.
pub(crate) fn tile_color_for(title: &str) -> slint::Color {
    let hash =
        title.bytes().fold(0u32, |acc, byte| acc.wrapping_mul(31).wrapping_add(u32::from(byte)));
    let [r, g, b] = TILE_PALETTE[(hash as usize) % TILE_PALETTE.len()];
    slint::Color::from_rgb_u8(r, g, b)
}

fn spacer_card() -> ExploreCardData {
    ExploreCardData {
        index: -1,
        title: "".into(),
        subtitle: "".into(),
        has_cover: false,
        cover: slint::Image::default(),
        tile_color: slint::Color::default(),
    }
}

fn pad_row_with_spacers(mut row: Vec<ExploreCardData>, columns: usize) -> ExploreCardRow {
    let padding = columns.saturating_sub(row.len());
    row.extend(std::iter::repeat_with(spacer_card).take(padding));
    ExploreCardRow { cards: ModelRc::new(VecModel::from(row)) }
}

/// Splits cards into fixed-width rows, padding the last row with invisible spacers.
pub(crate) fn chunk_card_rows(cards: Vec<ExploreCardData>, columns: usize) -> Vec<ExploreCardRow> {
    if cards.is_empty() {
        return Vec::new();
    }
    let cols = columns.max(1);
    let mut rows = Vec::new();
    let mut current_row = Vec::with_capacity(cols);
    for card in cards {
        current_row.push(card);
        if current_row.len() == cols {
            rows.push(ExploreCardRow {
                cards: ModelRc::new(VecModel::from(std::mem::replace(
                    &mut current_row,
                    Vec::with_capacity(cols),
                ))),
            });
        }
    }
    if !current_row.is_empty() {
        rows.push(pad_row_with_spacers(current_row, cols));
    }
    rows
}

fn explore_shelf_to_card_data(
    shelf: &Shelf,
    covers: &HashMap<String, slint::Image>,
    global_idx: &mut i32,
    columns: usize,
) -> ExploreShelfData {
    let mut cards = Vec::with_capacity(shelf.items.len());
    for item in &shelf.items {
        let row = SearchRow::from(item);
        let has_cover = row.artwork_url.is_some();
        let cover = resolve_row_cover_image(row.artwork_url.as_deref(), covers);
        let tile_color = tile_color_for(&row.title);
        cards.push(ExploreCardData {
            index: *global_idx,
            title: row.title.into(),
            subtitle: row.subtitle.into(),
            has_cover,
            cover,
            tile_color,
        });
        *global_idx += 1;
    }
    ExploreShelfData {
        title: shelf.title.clone().into(),
        rows: ModelRc::new(VecModel::from(chunk_card_rows(cards, columns))),
    }
}

/// Builds explore card shelves, keeping one global item index across all shelves.
pub(crate) fn explore_card_shelves(
    shelves: &[Shelf],
    covers: &HashMap<String, slint::Image>,
    columns: usize,
) -> Vec<ExploreShelfData> {
    let mut global_idx = 0;
    shelves
        .iter()
        .map(|s| explore_shelf_to_card_data(s, covers, &mut global_idx, columns))
        .collect()
}

/// Synchronizes explore shelves with Slint window models.
pub(crate) fn sync_explore_shelves<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    window: &MoggytWindow,
    state: &SlintAppState<C, A, S, L, K>,
) {
    window.set_explore_shelves(ModelRc::new(VecModel::from(explore_card_shelves(
        &state.explore_shelves,
        &state.search_covers,
        state.explore_columns,
    ))));
}

/// Periodic playback tick syncing elapsed timestamps, progress, and lyrics line without blocking.
pub(crate) fn sync_playback_tick<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    use crate::domain::{Album, Artist, Playlist, SearchResult, Track};
    use crate::slint_shell::platform::rgba_to_image;
    use crate::slint_shell::playback_worker::thumbnail_variant_for_row;

    fn make_test_card(index: i32) -> ExploreCardData {
        ExploreCardData {
            index,
            title: format!("Title {index}").into(),
            subtitle: "".into(),
            has_cover: false,
            cover: slint::Image::default(),
            tile_color: slint::Color::default(),
        }
    }

    #[test]
    fn test_explore_columns_for_width() {
        assert_eq!(explore_columns_for_width(1900), 10);
        assert_eq!(explore_columns_for_width(940), 4);
        assert_eq!(explore_columns_for_width(800), 3);
        assert_eq!(explore_columns_for_width(500), 2);
        assert_eq!(explore_columns_for_width(4000), 12);
    }

    #[test]
    fn test_chunk_card_rows_custom_columns() {
        // 0 cards -> no rows
        let rows_0 = chunk_card_rows(Vec::new(), 4);
        assert_eq!(rows_0.len(), 0);

        // 3 cards at 4 columns -> one row [0, 1, 2, -1]
        let cards_3 = vec![make_test_card(0), make_test_card(1), make_test_card(2)];
        let rows_1 = chunk_card_rows(cards_3, 4);
        assert_eq!(rows_1.len(), 1);
        assert_eq!(rows_1[0].cards.row_count(), 4);
        let indices_1: Vec<i32> =
            (0..4).map(|i| rows_1[0].cards.row_data(i).unwrap().index).collect();
        assert_eq!(indices_1, vec![0, 1, 2, -1]);

        // 7 cards at 4 columns -> two rows, second row padded to 4
        let cards_7: Vec<_> = (0..7).map(make_test_card).collect();
        let rows_2 = chunk_card_rows(cards_7, 4);
        assert_eq!(rows_2.len(), 2);
        assert_eq!(rows_2[0].cards.row_count(), 4);
        assert_eq!(rows_2[1].cards.row_count(), 4);
        let row0_indices: Vec<i32> =
            (0..4).map(|i| rows_2[0].cards.row_data(i).unwrap().index).collect();
        let row1_indices: Vec<i32> =
            (0..4).map(|i| rows_2[1].cards.row_data(i).unwrap().index).collect();
        assert_eq!(row0_indices, vec![0, 1, 2, 3]);
        assert_eq!(row1_indices, vec![4, 5, 6, -1]);
    }

    #[test]
    fn test_chunk_card_rows_padding_and_empty() {
        // 0 cards -> 0 rows
        let rows_0 = chunk_card_rows(Vec::new(), 5);
        assert_eq!(rows_0.len(), 0);

        // 3 cards -> 1 row of 5 with indexes [0, 1, 2, -1, -1]
        let cards_3 = vec![make_test_card(0), make_test_card(1), make_test_card(2)];
        let rows_1 = chunk_card_rows(cards_3, 5);
        assert_eq!(rows_1.len(), 1);
        assert_eq!(rows_1[0].cards.row_count(), 5);
        let indices_1: Vec<i32> =
            (0..5).map(|i| rows_1[0].cards.row_data(i).unwrap().index).collect();
        assert_eq!(indices_1, vec![0, 1, 2, -1, -1]);

        // 7 cards -> 2 rows of 5 with the second row padded
        let cards_7: Vec<_> = (0..7).map(make_test_card).collect();
        let rows_2 = chunk_card_rows(cards_7, 5);
        assert_eq!(rows_2.len(), 2);
        assert_eq!(rows_2[0].cards.row_count(), 5);
        assert_eq!(rows_2[1].cards.row_count(), 5);
        let row0_indices: Vec<i32> =
            (0..5).map(|i| rows_2[0].cards.row_data(i).unwrap().index).collect();
        let row1_indices: Vec<i32> =
            (0..5).map(|i| rows_2[1].cards.row_data(i).unwrap().index).collect();
        assert_eq!(row0_indices, vec![0, 1, 2, 3, 4]);
        assert_eq!(row1_indices, vec![5, 6, -1, -1, -1]);
    }

    #[test]
    fn test_explore_card_shelves_keeps_global_index_across_shelves() {
        let shelves = vec![
            Shelf {
                title: "Shelf 1".to_string(),
                items: vec![
                    SearchResult::Track(Track::new("t1", "Track 1", "Artist 1")),
                    SearchResult::Track(Track::new("t2", "Track 2", "Artist 2")),
                ],
            },
            Shelf {
                title: "Shelf 2".to_string(),
                items: vec![SearchResult::Track(Track::new("t3", "Track 3", "Artist 3"))],
            },
        ];
        let covers = HashMap::new();
        let result = explore_card_shelves(&shelves, &covers, 5);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title.as_str(), "Shelf 1");
        assert_eq!(result[1].title.as_str(), "Shelf 2");

        assert_eq!(result[0].rows.row_count(), 1);
        let row0 = result[0].rows.row_data(0).unwrap();
        assert_eq!(row0.cards.row_count(), 5);
        let row0_indices: Vec<i32> =
            (0..5).map(|i| row0.cards.row_data(i).unwrap().index).collect();
        assert_eq!(row0_indices, vec![0, 1, -1, -1, -1]);
        assert_eq!(row0.cards.row_data(0).unwrap().title.as_str(), "Track 1");
        assert_eq!(row0.cards.row_data(1).unwrap().title.as_str(), "Track 2");

        assert_eq!(result[1].rows.row_count(), 1);
        let row1 = result[1].rows.row_data(0).unwrap();
        assert_eq!(row1.cards.row_count(), 5);
        let row1_indices: Vec<i32> =
            (0..5).map(|i| row1.cards.row_data(i).unwrap().index).collect();
        assert_eq!(row1_indices, vec![2, -1, -1, -1, -1]);
        assert_eq!(row1.cards.row_data(0).unwrap().title.as_str(), "Track 3");
    }

    #[test]
    fn test_explore_card_shelves_subtitle_mapping_through_search_row() {
        let shelves = vec![Shelf {
            title: "Diverse".to_string(),
            items: vec![
                SearchResult::Track(Track::new("t", "Title", "Artist")),
                SearchResult::Album(Album::new("a", "Album", "Artist", None)),
                SearchResult::Artist(Artist::new("UC1", "Name", None)),
                SearchResult::Playlist(Playlist::new("VL1", "Mix", 0, None)),
            ],
        }];
        let covers = HashMap::new();
        let result = explore_card_shelves(&shelves, &covers, 5);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rows.row_count(), 1);
        let row0 = result[0].rows.row_data(0).unwrap();
        let cards = &row0.cards;
        assert_eq!(cards.row_count(), 5);

        assert_eq!(cards.row_data(0).unwrap().subtitle.as_str(), "Artist");
        assert_eq!(cards.row_data(1).unwrap().subtitle.as_str(), "Artist");
        assert_eq!(cards.row_data(2).unwrap().subtitle.as_str(), "");
        assert_eq!(cards.row_data(3).unwrap().subtitle.as_str(), "");
        assert_eq!(cards.row_data(4).unwrap().index, -1);
    }

    #[test]
    fn test_explore_card_shelves_has_cover_and_image_resolution() {
        let art_url = "https://example.test/art.jpg";
        let shelves = vec![Shelf {
            title: "Covers".to_string(),
            items: vec![
                SearchResult::Playlist(Playlist::new("VL1", "Calma", 0, None)),
                SearchResult::Album(Album::new("a", "Album", "Artist", None).with_artwork(art_url)),
            ],
        }];

        // Map is empty: has_cover false for playlist, true for album; album cover empty image
        let empty_covers = HashMap::new();
        let res_empty = explore_card_shelves(&shelves, &empty_covers, 5);
        let row_empty = res_empty[0].rows.row_data(0).unwrap();
        let pl_card = row_empty.cards.row_data(0).unwrap();
        let alb_card_empty = row_empty.cards.row_data(1).unwrap();

        assert!(!pl_card.has_cover);
        assert_eq!(pl_card.cover.size(), slint::Image::default().size());
        assert!(alb_card_empty.has_cover);
        assert_eq!(alb_card_empty.cover.size(), slint::Image::default().size());

        // Covers holds entry keyed by thumbnail_variant_for_row(url)
        let mut populated_covers = HashMap::new();
        let real_img = rgba_to_image(&[255, 0, 0, 255], 1, 1).expect("valid image");
        let thumb_url = thumbnail_variant_for_row(art_url);
        populated_covers.insert(thumb_url, real_img);

        let res_populated = explore_card_shelves(&shelves, &populated_covers, 5);
        let row_pop = res_populated[0].rows.row_data(0).unwrap();
        let alb_card_populated = row_pop.cards.row_data(1).unwrap();

        assert!(alb_card_populated.has_cover);
        assert!(alb_card_populated.cover.size().width > 0);
    }

    #[test]
    fn test_tile_color_for_deterministic_and_within_palette() {
        let color1 = tile_color_for("Chill Vibes");
        let color2 = tile_color_for("Chill Vibes");
        assert_eq!(color1, color2);

        let titles = [
            "Rock",
            "Pop",
            "Jazz",
            "Classical",
            "Hip Hop",
            "Electronic",
            "Acoustic",
            "Metal",
            "Ambient",
            "Dance",
            "Country",
            "Blues",
            "Folk",
            "Latin",
        ];
        let palette_colors: Vec<slint::Color> =
            TILE_PALETTE.iter().map(|&[r, g, b]| slint::Color::from_rgb_u8(r, g, b)).collect();

        for title in titles {
            let color = tile_color_for(title);
            assert!(
                palette_colors.contains(&color),
                "tile_color_for({title:?}) returned color not in TILE_PALETTE"
            );
        }
    }

    #[test]
    fn test_artless_item_keeps_has_cover_false_and_receives_tile_color() {
        let shelves = vec![Shelf {
            title: "Moods".to_string(),
            items: vec![SearchResult::Playlist(Playlist::new("PL1", "Energize", 0, None))],
        }];
        let covers = HashMap::new();
        let result = explore_card_shelves(&shelves, &covers, 5);

        assert_eq!(result.len(), 1);
        let row0 = result[0].rows.row_data(0).unwrap();
        let card0 = row0.cards.row_data(0).unwrap();

        assert_eq!(card0.title.as_str(), "Energize");
        assert!(!card0.has_cover);
        let expected_color = tile_color_for("Energize");
        assert_eq!(card0.tile_color, expected_color);

        // Verify trailing spacer cards
        let spacer = row0.cards.row_data(1).unwrap();
        assert_eq!(spacer.index, -1);
        assert!(!spacer.has_cover);
        assert_eq!(spacer.tile_color, slint::Color::default());
    }
}
