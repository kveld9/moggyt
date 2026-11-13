//! Explore section catalog querying, async loading, and navigation models.
//!
//! Hexagonal invariant: queries catalog port asynchronously off the UI thread and dispatches
//! results back to Slint via the event loop.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{SearchResult, Shelf};
use crate::infra::{Catalog, CatalogError};
use crate::slint_shell::covers::{
    dispatch_cover_with_generation_if_needed, iter_shelf_artwork_urls, request_explore_covers,
    resolve_row_cover_image,
};
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::search::SearchRow;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::{sync_category_shelves, sync_explore_shelves, sync_window};
use crate::slint_shell::view_phase::ViewPhase;
use crate::slint_shell::window::with_ui_state;
use crate::slint_shell::{ExploreCardData, ExploreCardRow, ExploreShelfData, MoggytWindow};

/// Queries catalog for explore shelves asynchronously off the UI thread.
pub fn execute_explore<T: ?Sized + Catalog>(
    generation: u64,
    catalog: &Arc<Mutex<T>>,
) -> (u64, Result<Vec<Shelf>, CatalogError>) {
    let result =
        catalog.lock().map_or(Err(CatalogError::Transport), |guard| guard.explore_shelves());
    (generation, result)
}

/// Queries catalog for category shelves asynchronously off the UI thread.
pub fn execute_category<T: ?Sized + Catalog>(
    generation: u64,
    catalog: &Arc<Mutex<T>>,
    browse_id: &str,
    params: Option<&str>,
) -> (u64, Result<Vec<Shelf>, CatalogError>) {
    let result = catalog
        .lock()
        .map_or(Err(CatalogError::Transport), |guard| guard.category_shelves(browse_id, params));
    (generation, result)
}

/// Helper resolving an item in explore shelves by linear index.
pub(crate) fn explore_item_at(shelves: &[Shelf], index: usize) -> Option<&SearchResult> {
    shelves.iter().flat_map(|s| s.items.iter()).nth(index)
}

/// Requests background cover decoding for all uncached artwork URLs in category shelves.
pub(crate) fn request_category_covers(
    state: &SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let mut seen = HashSet::new();
    for url in iter_shelf_artwork_urls(&state.category_shelves) {
        dispatch_cover_with_generation_if_needed(
            url,
            state.category_generation,
            &state.search_covers,
            tx,
            &mut seen,
        );
    }
}

/// Applies explore outcome (success or failure) directly to state.
pub(crate) fn apply_explore_outcome(
    st: &mut SlintAppState,
    result: Result<Vec<Shelf>, CatalogError>,
) {
    match result {
        Ok(shelves) => {
            st.explore_shelves = crate::domain::apply_shelf_policy(shelves, st.songs_only);
            let is_empty = st.explore_shelves.is_empty();
            st.explore_loaded = true;
            st.explore_loading = false;
            st.explore_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
        }
        Err(_) => {
            st.explore_loading = false;
            st.explore_phase = ViewPhase::Error;
        }
    }
}

/// Applies category outcome (success or failure) directly to state.
pub(crate) fn apply_category_outcome(
    st: &mut SlintAppState,
    result: Result<Vec<Shelf>, CatalogError>,
) {
    match result {
        Ok(shelves) => {
            st.category_shelves = crate::domain::apply_shelf_policy(shelves, st.songs_only);
            let is_empty = st.category_shelves.is_empty();
            st.category_loading = false;
            st.category_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
        }
        Err(_) => {
            st.category_loading = false;
            st.category_phase = ViewPhase::Error;
        }
    }
}

/// Applies completed explore shelves fetch to state and UI on main thread.
pub(crate) fn apply_explore_completion(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    generation: u64,
    result: Result<Vec<Shelf>, CatalogError>,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.explore_generation != generation {
        return;
    }
    let is_ok = result.is_ok();
    apply_explore_outcome(st, result);
    win.set_explore_loading(false);
    if is_ok {
        request_explore_covers(st, tx);
    }
    sync_explore_shelves(win, st);
}

/// Applies completed category shelves fetch to state and UI on main thread.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_category_completion(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    generation: u64,
    result: Result<Vec<Shelf>, CatalogError>,
    title: String,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.category_generation != generation {
        return;
    }
    st.category_title = title;
    let is_ok = result.is_ok();
    apply_category_outcome(st, result);
    win.set_category_loading(false);
    if is_ok {
        request_category_covers(st, tx);
    }
    sync_category_shelves(win, st);
}

/// Dispatches asynchronous explore shelves loading off the UI thread.
pub(crate) fn load_explore_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    state.explore_generation = state.explore_generation.wrapping_add(1);
    let generation = state.explore_generation;
    state.explore_loading = true;
    window.set_explore_loading(true);

    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();

    std::thread::spawn(move || {
        let (g, result) = execute_explore(generation, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state(|st| {
                apply_explore_completion(&win, st, g, result, &tx);
            });
        });
    });
}

/// Dispatches asynchronous category shelves loading off the UI thread.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_category_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    browse_id: &str,
    params: Option<&str>,
    title: &str,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if state.page != SlintPage::Category {
        state.category_return = state.page;
    }
    state.page = SlintPage::Category;
    state.category_title = title.to_owned();
    state.category_shelves.clear();
    state.category_generation = state.category_generation.wrapping_add(1);
    let generation = state.category_generation;
    state.category_loading = true;
    window.set_category_loading(true);
    sync_window(window, state);

    let (cat, weak, tx) = (Arc::clone(&state.catalog), window.as_weak(), worker_tx.clone());
    let (id, p, t) = (browse_id.to_string(), params.map(str::to_string), title.to_string());

    std::thread::spawn(move || {
        let (g, result) = execute_category(generation, &cat, &id, p.as_deref());
        let _ = dispatch_to_ui(move || {
            if let Some(win) = weak.upgrade() {
                with_ui_state(|st| apply_category_completion(&win, st, g, result, t, &tx));
            }
        });
    });
}

/// Re-dispatches explore cover fetches when Explore is re-entered with shelves already
/// loaded. Idempotent: `request_explore_covers` only dispatches URLs missing from the
/// UI cover cache. Returns false when there is nothing to refresh.
pub(crate) fn refresh_explore_covers_if_loaded(
    st: &SlintAppState,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) -> bool {
    if !st.explore_loaded || st.explore_shelves.is_empty() {
        return false;
    }
    request_explore_covers(st, tx);
    true
}

/// Card width in logical pixels; must match `width` in `ui/components/cards.slint`.
pub(crate) const EXPLORE_CARD_WIDTH: i32 = 150;
/// Horizontal gap between cards in logical pixels.
pub(crate) const EXPLORE_CARD_GAP: i32 = 12;
/// Horizontal padding of the content column (24px per side in `ui/views/content_area.slint`).
pub(crate) const EXPLORE_CONTENT_PADDING: i32 = 48;
/// Column bounds: a single column is the only fit in a content area squeezed by the queue
/// panel, and 12 caps the grid on very wide windows.
const MIN_EXPLORE_COLUMNS: usize = 1;
const MAX_EXPLORE_COLUMNS: usize = 12;

/// Number of cards that fit in one row for a given content-area width.
pub(crate) fn explore_columns_for_width(content_width: i32) -> usize {
    let usable = content_width - EXPLORE_CONTENT_PADDING + EXPLORE_CARD_GAP;
    let per_card = EXPLORE_CARD_WIDTH + EXPLORE_CARD_GAP;
    if usable <= 0 {
        return MIN_EXPLORE_COLUMNS;
    }
    ((usable / per_card) as usize).clamp(MIN_EXPLORE_COLUMNS, MAX_EXPLORE_COLUMNS)
}

/// Stores the card column count for a content-area width.
///
/// Returns true only when the count changed, so callers re-chunk the card models once per
/// threshold crossing instead of on every animation frame of the queue panel.
pub(crate) fn apply_content_width(state: &mut SlintAppState, content_width: i32) -> bool {
    let columns = explore_columns_for_width(content_width);
    if state.explore_columns == columns {
        return false;
    }
    state.explore_columns = columns;
    true
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

#[cfg(test)]
#[path = "explore_tests.rs"]
mod tests;
