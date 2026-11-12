//! Right-click context menu for track rows and container cards: target resolution and action
//! dispatch.
//!
//! Hexagonal invariant: pure in-memory target resolution and queue mutations; this module
//! performs no I/O, no network access, and never reads clocks.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{DetailCardKind, DetailKind, SearchResult, Track};
use crate::i18n::{Locale, tr};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::explore::explore_item_at;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::playback::is_track_liked;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::search::iter_shelf_items;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::sync_window;
use crate::slint_shell::toast::notify;
use crate::slint_shell::window::{open_detail_async, trigger_playback};
use crate::slint_shell::{
    ContextIcon, ContextMenuItemData, ContextSurface, ContextUi, MoggytWindow,
};

/// Track actions offered by the context menu, in rendering order.
pub const TRACK_ACTIONS: [ContextAction; 5] = [
    ContextAction::PlayNow,
    ContextAction::PlayNext,
    ContextAction::AddToQueue,
    ContextAction::ToggleLike,
    ContextAction::CopyLink,
];

/// Actions offered for container rows, in rendering order.
pub const CONTAINER_ACTIONS: [ContextAction; 1] = [ContextAction::Open];

/// Subject addressed by an open context menu.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextSubject {
    /// A single playable track.
    Track(Track),
    /// A browsable album, artist, or playlist.
    Container { id: String, kind: DetailKind },
}

impl ContextSubject {
    /// Actions offered for this subject, in rendering order.
    pub fn actions(&self) -> &'static [ContextAction] {
        match self {
            Self::Track(_) => &TRACK_ACTIONS,
            Self::Container { .. } => &CONTAINER_ACTIONS,
        }
    }

    /// Maps a rendered menu index back to an action for this subject.
    pub fn action_at(&self, index: i32) -> Option<ContextAction> {
        usize::try_from(index).ok().and_then(|i| self.actions().get(i)).copied()
    }
}

/// Action a context menu entry performs.
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
    /// Open the container detail page.
    Open,
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
            Self::Open => tr(locale, "Open"),
        }
    }

    /// Icon shown next to the action label.
    pub fn icon(self, liked: bool) -> ContextIcon {
        match self {
            Self::PlayNow => ContextIcon::Play,
            Self::PlayNext => ContextIcon::Next,
            Self::AddToQueue => ContextIcon::Queue,
            Self::ToggleLike if liked => ContextIcon::HeartFilled,
            Self::ToggleLike => ContextIcon::Heart,
            Self::CopyLink => ContextIcon::Copy,
            Self::Open => ContextIcon::Open,
        }
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

/// Resolves the subject a context target points at.
pub fn resolve_subject(state: &SlintAppState, target: ContextTarget) -> Option<ContextSubject> {
    match target.surface {
        ContextSurface::Search | ContextSurface::Explore | ContextSurface::Category => {
            item_subject(state, target)
        }
        ContextSurface::Detail | ContextSurface::Queue => track_subject(state, target),
        ContextSurface::DetailCard => detail_card_subject(state, target.index),
        ContextSurface::LibraryPlaylist | ContextSurface::LibraryAlbum => {
            library_subject(state, target)
        }
    }
}

fn item_subject(state: &SlintAppState, target: ContextTarget) -> Option<ContextSubject> {
    let item = match target.surface {
        ContextSurface::Search => {
            iter_shelf_items(&state.home_shelves, state.search_filter).nth(target.index)?
        }
        ContextSurface::Explore => explore_item_at(&state.explore_shelves, target.index)?,
        ContextSurface::Category => explore_item_at(&state.category_shelves, target.index)?,
        _ => return None,
    };
    subject_from_item(item)
}

fn track_subject(state: &SlintAppState, target: ContextTarget) -> Option<ContextSubject> {
    let track = match target.surface {
        ContextSurface::Detail => state.detail.as_ref()?.tracks.get(target.index)?,
        ContextSurface::Queue => state.queue.tracks().get(target.index)?,
        _ => return None,
    };
    Some(ContextSubject::Track(track.clone()))
}

fn library_subject(state: &SlintAppState, target: ContextTarget) -> Option<ContextSubject> {
    match target.surface {
        ContextSurface::LibraryPlaylist => {
            let playlist = state.library_playlists.get(target.index)?;
            Some(container_subject(&playlist.id, DetailKind::Playlist))
        }
        ContextSurface::LibraryAlbum => {
            let album = state.library_albums.get(target.index)?;
            Some(container_subject(&album.id, DetailKind::Album))
        }
        _ => None,
    }
}

fn detail_card_subject(state: &SlintAppState, index: usize) -> Option<ContextSubject> {
    let page = state.detail.as_ref()?.artist.as_ref()?;
    let mut flat = 0usize;
    for section in &page.sections {
        for card in &section.cards {
            if flat == index {
                return Some(container_subject(&card.id, card_detail_kind(card.kind)));
            }
            flat += 1;
        }
    }
    None
}

fn card_detail_kind(kind: DetailCardKind) -> DetailKind {
    match kind {
        DetailCardKind::Album => DetailKind::Album,
        DetailCardKind::Artist => DetailKind::Artist,
    }
}

fn container_subject(id: &str, kind: DetailKind) -> ContextSubject {
    ContextSubject::Container { id: id.to_owned(), kind }
}

fn subject_from_item(item: &SearchResult) -> Option<ContextSubject> {
    match item {
        SearchResult::Track(track) => Some(ContextSubject::Track(track.clone())),
        SearchResult::Album(album) => Some(container_subject(&album.id, DetailKind::Album)),
        SearchResult::Artist(artist) => Some(container_subject(&artist.id, DetailKind::Artist)),
        SearchResult::Playlist(playlist) => {
            Some(container_subject(&playlist.id, DetailKind::Playlist))
        }
    }
}

/// Builds the Slint menu model for a resolved subject.
pub fn menu_items(
    locale: Locale,
    subject: &ContextSubject,
    liked: bool,
) -> Vec<ContextMenuItemData> {
    subject
        .actions()
        .iter()
        .enumerate()
        .map(|(index, action)| ContextMenuItemData {
            index: index as i32,
            label: action.label(locale, liked).into(),
            icon: action.icon(liked),
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
        ContextAction::Open => FollowUp::None,
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
    let Some(subject) = resolve_subject(state, target) else {
        close_menu(window, state);
        return;
    };
    let liked = match &subject {
        ContextSubject::Track(track) => is_track_liked(&state.liked_ids, &track.id),
        ContextSubject::Container { .. } => false,
    };
    state.context_menu.open(target);
    let ui = window.global::<ContextUi>();
    ui.set_items(ModelRc::new(VecModel::from(menu_items(state.locale, &subject, liked))));
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
    let Some(subject) = resolve_subject(state, target) else {
        return;
    };
    let Some(action) = subject.action_at(item) else {
        return;
    };
    match subject {
        ContextSubject::Track(track) => {
            request_queue_track_cover(
                track.artwork.as_deref(),
                state.play_generation,
                &state.search_covers,
                worker_tx,
            );
            finish_track(window, state, target, action, track);
        }
        ContextSubject::Container { id, kind } => {
            if action == ContextAction::Open {
                open_detail_async(window, state, &id, kind, worker_tx);
            }
        }
    }
}

fn finish_track(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    target: ContextTarget,
    action: ContextAction,
    track: Track,
) {
    let track_id = track.id.clone();
    let follow_up = apply_action(state, target, action, track);
    let message = match follow_up {
        FollowUp::StartPlayback => {
            trigger_playback(window, state);
            None
        }
        FollowUp::CopyLink(url) => Some(if copy_track_link(&url) {
            (tr(state.locale, "Link copied"), ContextIcon::Copy)
        } else {
            (tr(state.locale, "Could not copy the link"), ContextIcon::Close)
        }),
        FollowUp::None => action_message(state, action, &track_id),
    };
    if let Some((text, icon)) = message {
        notify(window, state, text, icon);
    }
    sync_window(window, state);
}

fn action_message(
    state: &SlintAppState,
    action: ContextAction,
    track_id: &str,
) -> Option<(&'static str, ContextIcon)> {
    match action {
        ContextAction::PlayNext => Some((tr(state.locale, "Playing next"), ContextIcon::Next)),
        ContextAction::AddToQueue => Some((tr(state.locale, "Added to queue"), ContextIcon::Queue)),
        ContextAction::ToggleLike if is_track_liked(&state.liked_ids, track_id) => {
            Some((tr(state.locale, "Added to favorites"), ContextIcon::HeartFilled))
        }
        ContextAction::ToggleLike => {
            Some((tr(state.locale, "Removed from favorites"), ContextIcon::Heart))
        }
        _ => None,
    }
}

fn copy_track_link(url: &str) -> bool {
    match crate::infra::copy_text(url) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!("failed to copy track link: {err}");
            false
        }
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
#[path = "context_menu_tests.rs"]
mod tests;
