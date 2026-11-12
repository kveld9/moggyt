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
use crate::slint_shell::window::{open_detail_async, trigger_playback};
use crate::slint_shell::{ContextMenuItemData, ContextSurface, ContextUi, MoggytWindow};

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
    use crate::domain::{
        Album, Artist, ArtistPage, Detail, DetailCard, DetailCardKind, DetailKind, DetailSection,
        Playlist, Shelf,
    };
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn menu_items_follow_track_action_order() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let items = menu_items(Locale::English, &subject, false);
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
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let items = menu_items(Locale::Spanish, &subject, false);
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
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        let en_items = menu_items(Locale::English, &subject, true);
        assert_eq!(en_items.get(3).map(|item| item.label.as_str()), Some("Unlike"));

        let es_items = menu_items(Locale::Spanish, &subject, true);
        assert_eq!(es_items.get(3).map(|item| item.label.as_str()), Some("Quitar me gusta"));
    }

    #[test]
    fn action_index_mapping_rejects_out_of_range() {
        let subject = ContextSubject::Track(Track::new("t1", "Title", "Artist"));
        assert_eq!(subject.action_at(0), Some(ContextAction::PlayNow));
        assert_eq!(subject.action_at(1), Some(ContextAction::PlayNext));
        assert_eq!(subject.action_at(2), Some(ContextAction::AddToQueue));
        assert_eq!(subject.action_at(3), Some(ContextAction::ToggleLike));
        assert_eq!(subject.action_at(4), Some(ContextAction::CopyLink));
        assert_eq!(subject.action_at(-1), None);
        assert_eq!(subject.action_at(5), None);
    }

    #[test]
    fn container_menu_offers_only_open() {
        let container = ContextSubject::Container { id: "a1".to_owned(), kind: DetailKind::Album };
        let en_items = menu_items(Locale::English, &container, false);
        assert_eq!(en_items.len(), 1);
        assert_eq!(en_items[0].index, 0);
        assert_eq!(en_items[0].label, "Open");

        let es_items = menu_items(Locale::Spanish, &container, false);
        assert_eq!(es_items.len(), 1);
        assert_eq!(es_items[0].index, 0);
        assert_eq!(es_items[0].label, "Abrir");
    }

    #[test]
    fn resolve_subject_reads_search_items() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        let album = Album::new("a1", "Album 1", "Artist 1", None);
        let mut shelf = Shelf::new("Results");
        shelf.items.push(SearchResult::Track(track.clone()));
        shelf.items.push(SearchResult::Album(album));
        state.home_shelves = vec![shelf];

        let target_track = ContextTarget { surface: ContextSurface::Search, index: 0 };
        assert_eq!(resolve_subject(&state, target_track), Some(ContextSubject::Track(track)));

        let target_album = ContextTarget { surface: ContextSurface::Search, index: 1 };
        assert_eq!(
            resolve_subject(&state, target_album),
            Some(ContextSubject::Container { id: "a1".to_owned(), kind: DetailKind::Album })
        );
    }

    #[test]
    fn resolve_subject_reads_detail_tracks() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist 1");
        state.detail = Some(Detail::new(DetailKind::Album, "album", "artist", vec![track.clone()]));

        let target = ContextTarget { surface: ContextSurface::Detail, index: 0 };
        assert_eq!(resolve_subject(&state, target), Some(ContextSubject::Track(track)));
    }

    #[test]
    fn resolve_subject_reads_explore_and_category_shelves() {
        let mut state = test_wired();
        let artist = Artist::new("art1", "Artist 1", None);
        let playlist = Playlist::new("pl1", "Playlist 1", 10, None);

        let mut explore_shelf = Shelf::new("Explore");
        explore_shelf.items.push(SearchResult::Artist(artist));
        state.explore_shelves = vec![explore_shelf];

        let mut category_shelf = Shelf::new("Category");
        category_shelf.items.push(SearchResult::Playlist(playlist));
        state.category_shelves = vec![category_shelf];

        let target_explore = ContextTarget { surface: ContextSurface::Explore, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_explore),
            Some(ContextSubject::Container { id: "art1".to_owned(), kind: DetailKind::Artist })
        );

        let target_category = ContextTarget { surface: ContextSurface::Category, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_category),
            Some(ContextSubject::Container { id: "pl1".to_owned(), kind: DetailKind::Playlist })
        );
    }

    #[test]
    fn resolve_subject_reads_library_items() {
        let mut state = test_wired();
        state.library_playlists = vec![Playlist::new("pl1", "Playlist 1", 5, None)];
        state.library_albums = vec![Album::new("al1", "Album 1", "Artist 1", None)];

        let target_playlist = ContextTarget { surface: ContextSurface::LibraryPlaylist, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_playlist),
            Some(ContextSubject::Container { id: "pl1".to_owned(), kind: DetailKind::Playlist })
        );

        let target_album = ContextTarget { surface: ContextSurface::LibraryAlbum, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_album),
            Some(ContextSubject::Container { id: "al1".to_owned(), kind: DetailKind::Album })
        );
    }

    #[test]
    fn resolve_subject_reads_detail_section_cards() {
        let mut state = test_wired();
        let card_album =
            DetailCard::new("c-album", "Album Title", "Sub", None, DetailCardKind::Album);
        let card_artist =
            DetailCard::new("c-artist", "Artist Title", "Sub", None, DetailCardKind::Artist);

        let section_1 = DetailSection { title: "Albums".to_owned(), cards: vec![card_album] };
        let section_2 =
            DetailSection { title: "Related Artists".to_owned(), cards: vec![card_artist] };

        let artist_page = ArtistPage {
            songs_title: "Top Songs".to_owned(),
            songs_more_id: None,
            sections: vec![section_1, section_2],
        };

        let mut detail = Detail::new(DetailKind::Artist, "Artist Name", "Description", vec![]);
        detail = detail.with_artist_page(artist_page);
        state.detail = Some(detail);

        let target_0 = ContextTarget { surface: ContextSurface::DetailCard, index: 0 };
        assert_eq!(
            resolve_subject(&state, target_0),
            Some(ContextSubject::Container { id: "c-album".to_owned(), kind: DetailKind::Album })
        );

        let target_1 = ContextTarget { surface: ContextSurface::DetailCard, index: 1 };
        assert_eq!(
            resolve_subject(&state, target_1),
            Some(ContextSubject::Container { id: "c-artist".to_owned(), kind: DetailKind::Artist })
        );

        let target_2 = ContextTarget { surface: ContextSurface::DetailCard, index: 2 };
        assert_eq!(resolve_subject(&state, target_2), None);
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
