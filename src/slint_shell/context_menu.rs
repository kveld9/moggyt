//! Right-click context menu for track rows: target resolution and action dispatch.
//!
//! Hexagonal invariant: pure in-memory target resolution and queue mutations; this module
//! performs no I/O, no network access, and never reads clocks.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{SearchResult, Track};
use crate::i18n::{Locale, tr};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::search::iter_shelf_items;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::sync_window;
use crate::slint_shell::window::trigger_playback;
use crate::slint_shell::{ContextMenuItemData, ContextSurface, ContextUi, MoggytWindow};

/// Track actions offered by the context menu, in rendering order.
pub const TRACK_ACTIONS: [ContextAction; 5] = [
    ContextAction::PlayNow,
    ContextAction::PlayNext,
    ContextAction::AddToQueue,
    ContextAction::ToggleLike,
    ContextAction::CopyLink,
];

/// Action a track-row context menu entry performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    /// Queue the track and start playback on it.
    PlayNow,
    /// Insert the track right after the current queue position.
    PlayNext,
    /// Append the track to the end of the queue.
    AddToQueue,
    /// Toggle the local favorite flag for the track.
    ToggleLike,
    /// Copy the YouTube Music watch URL for the track.
    CopyLink,
}

impl ContextAction {
    /// Localized menu label for the action.
    pub fn label(self, locale: Locale, liked: bool) -> &'static str {
        match self {
            Self::PlayNow => tr(locale, "Play"),
            Self::PlayNext => tr(locale, "Play next"),
            Self::AddToQueue => tr(locale, "Add to queue"),
            Self::ToggleLike if liked => tr(locale, "Unlike"),
            Self::ToggleLike => tr(locale, "Like"),
            Self::CopyLink => tr(locale, "Copy link"),
        }
    }

    /// Maps a rendered menu index back to its action.
    pub fn from_index(index: i32) -> Option<Self> {
        usize::try_from(index).ok().and_then(|i| TRACK_ACTIONS.get(i)).copied()
    }
}

/// Row that owns the currently open context menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextTarget {
    /// List the row belongs to.
    pub surface: ContextSurface,
    /// Row position inside that list.
    pub index: usize,
}

/// Open-menu bookkeeping stored with the rest of the UI state.
#[derive(Debug, Default)]
pub struct ContextMenuState {
    target: Option<ContextTarget>,
}

impl ContextMenuState {
    /// Records the row that owns the menu.
    pub fn open(&mut self, target: ContextTarget) {
        self.target = Some(target);
    }

    /// Forgets the owning row.
    pub fn close(&mut self) {
        self.target = None;
    }

    /// Row that owns the menu, when open.
    pub fn target(&self) -> Option<ContextTarget> {
        self.target
    }
}

/// Window work requested after a queue mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUp {
    /// No playback or view change is needed.
    None,
    /// Playback must be (re)triggered for the track under the cursor.
    StartPlayback,
    /// The derived watch URL must be written to the system clipboard.
    CopyLink(String),
}

/// Resolves the track a context target points at, when the row is a track.
pub fn resolve_track(state: &SlintAppState, target: ContextTarget) -> Option<Track> {
    match target.surface {
        ContextSurface::Search => {
            match iter_shelf_items(&state.home_shelves, state.search_filter).nth(target.index)? {
                SearchResult::Track(track) => Some(track.clone()),
                _ => None,
            }
        }
        ContextSurface::Detail => state.detail.as_ref()?.tracks.get(target.index).cloned(),
        ContextSurface::Queue => state.queue.tracks().get(target.index).cloned(),
    }
}

/// Builds the Slint menu model for a track row.
pub fn menu_items(locale: Locale, liked: bool) -> Vec<ContextMenuItemData> {
    TRACK_ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| ContextMenuItemData {
            index: index as i32,
            label: action.label(locale, liked).into(),
        })
        .collect()
}

/// Applies a menu action to the queue. Window effects are returned as a [`FollowUp`].
pub fn apply_action(
    state: &mut SlintAppState,
    target: ContextTarget,
    action: ContextAction,
    track: Track,
) -> FollowUp {
    match action {
        ContextAction::AddToQueue => {
            state.queue.push(track);
            FollowUp::None
        }
        ContextAction::PlayNext => {
            state.queue.insert_next(track);
            FollowUp::None
        }
        ContextAction::PlayNow => play_now(state, target, track),
        ContextAction::ToggleLike => {
            super::likes::toggle_and_persist(state, &track.id);
            FollowUp::None
        }
        ContextAction::CopyLink => {
            match crate::slint_shell::import_open::youtube_music_url(&track.id) {
                Some(url) => FollowUp::CopyLink(url),
                None => FollowUp::None,
            }
        }
    }
}

fn play_now(state: &mut SlintAppState, target: ContextTarget, track: Track) -> FollowUp {
    if target.surface == ContextSurface::Queue {
        state.play_track_at_index_in_memory(target.index);
        return FollowUp::StartPlayback;
    }
    state.queue.push(track);
    let index = state.queue.len().saturating_sub(1);
    state.play_track_at_index_in_memory(index);
    state.page = SlintPage::NowPlaying;
    FollowUp::StartPlayback
}

/// Pointer request emitted by a right-clicked track row.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MenuRequest {
    surface: ContextSurface,
    index: i32,
    x: f32,
    y: f32,
}

fn open_menu(window: &MoggytWindow, state: &mut SlintAppState, request: MenuRequest) {
    let Ok(index) = usize::try_from(request.index) else {
        close_menu(window, state);
        return;
    };
    let target = ContextTarget { surface: request.surface, index };
    let Some(track) = resolve_track(state, target) else {
        close_menu(window, state);
        return;
    };
    let liked = crate::slint_shell::playback::is_track_liked(&state.liked_ids, &track.id);
    state.context_menu.open(target);
    let ui = window.global::<ContextUi>();
    ui.set_items(ModelRc::new(VecModel::from(menu_items(state.locale, liked))));
    ui.set_x(request.x);
    ui.set_y(request.y);
    ui.set_is_open(true);
}

fn close_menu(window: &MoggytWindow, state: &mut SlintAppState) {
    state.context_menu.close();
    window.global::<ContextUi>().set_is_open(false);
}

fn activate(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    item: i32,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(target) = state.context_menu.target() else {
        return;
    };
    close_menu(window, state);
    let (Some(action), Some(track)) =
        (ContextAction::from_index(item), resolve_track(state, target))
    else {
        return;
    };
    request_queue_track_cover(
        track.artwork.as_deref(),
        state.play_generation,
        &state.search_covers,
        worker_tx,
    );
    match apply_action(state, target, action, track) {
        FollowUp::StartPlayback => trigger_playback(window, state),
        FollowUp::CopyLink(url) => copy_track_link(&url),
        FollowUp::None => {}
    }
    sync_window(window, state);
}

fn copy_track_link(url: &str) {
    if let Err(err) = crate::infra::copy_text(url) {
        tracing::warn!("failed to copy track link: {err}");
    }
}

/// Wires the context menu global callbacks to the window and application state.
pub(crate) fn wire(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<ContextUi>().on_opened(move |surface, index, x, y| {
        let Some(win) = w.upgrade() else {
            return;
        };
        let mut st = s.borrow_mut();
        open_menu(&win, &mut st, MenuRequest { surface, index, x, y });
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.global::<ContextUi>().on_item_activated(move |item| {
        let Some(win) = w.upgrade() else {
            return;
        };
        let mut st = s.borrow_mut();
        activate(&win, &mut st, item, &tx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.global::<ContextUi>().on_dismissed(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let mut st = s.borrow_mut();
        close_menu(&win, &mut st);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Detail, DetailKind, Shelf};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn menu_items_follow_track_action_order() {
        let items = menu_items(Locale::English, false);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].index, 0);
        assert_eq!(items[0].label, "Play");
        assert_eq!(items[1].index, 1);
        assert_eq!(items[1].label, "Play next");
        assert_eq!(items[2].index, 2);
        assert_eq!(items[2].label, "Add to queue");
        assert_eq!(items[3].index, 3);
        assert_eq!(items[3].label, "Like");
        assert_eq!(items[4].index, 4);
        assert_eq!(items[4].label, "Copy link");
    }

    #[test]
    fn menu_items_are_localized() {
        let items = menu_items(Locale::Spanish, false);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Reproducir",
                "Reproducir a continuación",
                "Añadir a la cola",
                "Me gusta",
                "Copiar enlace",
            ]
        );
    }

    #[test]
    fn menu_items_switch_like_label_when_liked() {
        let en_items = menu_items(Locale::English, true);
        assert_eq!(en_items.get(3).map(|item| item.label.as_str()), Some("Unlike"));

        let es_items = menu_items(Locale::Spanish, true);
        assert_eq!(es_items.get(3).map(|item| item.label.as_str()), Some("Quitar me gusta"));
    }

    #[test]
    fn action_index_mapping_rejects_out_of_range() {
        assert_eq!(ContextAction::from_index(0), Some(ContextAction::PlayNow));
        assert_eq!(ContextAction::from_index(1), Some(ContextAction::PlayNext));
        assert_eq!(ContextAction::from_index(2), Some(ContextAction::AddToQueue));
        assert_eq!(ContextAction::from_index(3), Some(ContextAction::ToggleLike));
        assert_eq!(ContextAction::from_index(4), Some(ContextAction::CopyLink));
        assert_eq!(ContextAction::from_index(-1), None);
        assert_eq!(ContextAction::from_index(5), None);
    }

    #[test]
    fn resolve_track_reads_search_shelves() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        let album = Album::new("a1", "Album 1", "Artist 1", None);
        let mut shelf = Shelf::new("Results");
        shelf.items.push(SearchResult::Track(track.clone()));
        shelf.items.push(SearchResult::Album(album));
        state.home_shelves = vec![shelf];

        let target_track = ContextTarget { surface: ContextSurface::Search, index: 0 };
        assert_eq!(resolve_track(&state, target_track), Some(track));

        let target_album = ContextTarget { surface: ContextSurface::Search, index: 1 };
        assert_eq!(resolve_track(&state, target_album), None);
    }

    #[test]
    fn resolve_track_reads_detail_tracks() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        state.detail = Some(Detail::new(DetailKind::Album, "album", "artist", vec![track.clone()]));

        let target = ContextTarget { surface: ContextSurface::Detail, index: 0 };
        assert_eq!(resolve_track(&state, target), Some(track));
    }

    #[test]
    fn apply_action_play_next_inserts_after_cursor() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));
        state.queue.push(Track::new("b", "Title B", "Artist B"));
        state.queue.push(Track::new("c", "Title C", "Artist C"));
        state.queue.advance();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("b"));

        let track_x = Track::new("x", "Title X", "Artist X");
        let target = ContextTarget { surface: ContextSurface::Queue, index: 1 };
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNext, track_x);

        assert_eq!(follow_up, FollowUp::None);
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "x", "c"]);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("b"));
    }

    #[test]
    fn apply_action_add_to_queue_appends() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));

        let track_x = Track::new("x", "Title X", "Artist X");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::AddToQueue, track_x);

        assert_eq!(follow_up, FollowUp::None);
        let ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "x"]);
    }

    #[test]
    fn apply_action_play_now_on_queue_surface_moves_cursor() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Title A", "Artist A"));
        state.queue.push(Track::new("b", "Title B", "Artist B"));
        assert_eq!(state.queue.cursor(), 0);

        let target = ContextTarget { surface: ContextSurface::Queue, index: 1 };
        let track_b = state.queue.tracks()[1].clone();
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNow, track_b);

        assert_eq!(follow_up, FollowUp::StartPlayback);
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.queue.len(), 2);
    }

    #[test]
    fn apply_action_play_now_on_detail_pushes_and_opens_now_playing() {
        let mut state = test_wired();
        assert_eq!(state.queue.len(), 0);

        let track = Track::new("t1", "Title 1", "Artist 1");
        let target = ContextTarget { surface: ContextSurface::Detail, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::PlayNow, track);

        assert_eq!(follow_up, FollowUp::StartPlayback);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.page, SlintPage::NowPlaying);
    }

    #[test]
    fn apply_action_toggle_like_updates_set_and_settings() {
        let mut state = test_wired();
        let track = Track::new("t1", "Title 1", "Artist 1");
        let target = ContextTarget { surface: ContextSurface::Queue, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::ToggleLike, track);

        assert_eq!(follow_up, FollowUp::None);
        assert!(state.liked_ids.contains("t1"));
        assert_eq!(state.settings_store.load().liked_ids, vec!["t1"]);
    }

    #[test]
    fn apply_action_copy_link_returns_the_watch_url() {
        let mut state = test_wired();
        let track = Track::new("abc-123_XYZ", "Title", "Artist");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::CopyLink, track);

        assert_eq!(
            follow_up,
            FollowUp::CopyLink("https://music.youtube.com/watch?v=abc-123_XYZ".to_owned())
        );
        assert_eq!(state.queue.len(), 0);
    }

    #[test]
    fn apply_action_copy_link_skips_invalid_identifiers() {
        let mut state = test_wired();
        let track = Track::new("bad id", "Title", "Artist");
        let target = ContextTarget { surface: ContextSurface::Search, index: 0 };
        let follow_up = apply_action(&mut state, target, ContextAction::CopyLink, track);

        assert_eq!(follow_up, FollowUp::None);
    }
}
