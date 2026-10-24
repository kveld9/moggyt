//! UI shell: navigation state and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.

use eframe::egui;

use crate::domain::{PlaybackQueue, SearchQuery};
use crate::infra::{AudioBackend, Catalog};

/// Navigation pages of the application shell.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// Search the catalog.
    #[default]
    Search,
    /// Inspect and reorder the playback queue.
    Queue,
    /// Currently playing view.
    NowPlaying,
}

/// Wiring between the UI shell and its collaborators.
pub struct AppState<C: Catalog, A: AudioBackend> {
    /// Active navigation page.
    pub page: Page,
    /// Pure playback queue (no I/O).
    pub queue: PlaybackQueue,
    /// Text typed into the search box (UI state only).
    pub search_text: String,
    /// Catalog adapter (injected port).
    pub catalog: C,
    /// Audio adapter (injected port).
    pub audio: A,
}

impl<C: Catalog, A: AudioBackend> AppState<C, A> {
    /// Creates the shell with injected collaborators.
    pub fn new(catalog: C, audio: A) -> Self {
        Self {
            page: Page::default(),
            queue: PlaybackQueue::new(),
            search_text: String::new(),
            catalog,
            audio,
        }
    }

    /// Handles a search submission: parses the raw input at the boundary,
    /// queries the catalog, and enqueues the first hit.
    pub fn submit_search(&mut self) {
        let Some(query) = SearchQuery::parse(&self.search_text) else {
            return;
        };
        let hits = self.catalog.search(query.text());
        if let Some(track) = hits.into_iter().next() {
            self.queue.push(track);
            self.page = Page::Queue;
        }
    }

    /// Starts playback of the track at the head of the queue.
    pub fn play_head(&mut self) {
        if let Some(track) = self.queue.current() {
            let id = track.id.clone();
            self.audio.play(&id);
            self.page = Page::NowPlaying;
        }
    }
}

/// eframe application wrapper (thin view over [`AppState`]).
pub struct MoggytApp<C: Catalog, A: AudioBackend> {
    state: AppState<C, A>,
}

impl<C: Catalog, A: AudioBackend> MoggytApp<C, A> {
    /// Wraps an already-wired state for eframe.
    pub fn new(state: AppState<C, A>) -> Self {
        Self { state }
    }
}

impl<C: Catalog, A: AudioBackend> eframe::App for MoggytApp<C, A> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        render_top_bar(ui, &mut self.state);
        render_body(ui, &mut self.state);
    }
}

fn render_top_bar<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &mut AppState<C, A>) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("moggyt");
            ui.separator();
            ui.selectable_value(&mut state.page, Page::Search, "Search");
            ui.selectable_value(&mut state.page, Page::Queue, "Queue");
            ui.selectable_value(&mut state.page, Page::NowPlaying, "Now playing");
        });
    });
}

fn render_body<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &mut AppState<C, A>) {
    egui::CentralPanel::default().show(ui, |ui| match state.page {
        Page::Search => render_search(ui, state),
        Page::Queue => render_queue(ui, state),
        Page::NowPlaying => render_now_playing(ui, state),
    });
}

fn render_search<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &mut AppState<C, A>) {
    ui.label("Search the catalog (stub returns fixed hits):");
    let submitted = ui.text_edit_singleline(&mut state.search_text).lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if ui.button("Search").clicked() || submitted {
        state.submit_search();
    }
}

fn render_queue<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &mut AppState<C, A>) {
    ui.label(format!("{} track(s) queued", state.queue.len()));
    ui.horizontal(|ui| {
        if ui.button("Previous").clicked() {
            state.queue.retreat();
        }
        if ui.button("Play head").clicked() {
            state.play_head();
        }
        if ui.button("Next").clicked() {
            state.queue.advance();
        }
    });
    for track in state.queue.tracks() {
        ui.label(track.display());
    }
}

fn render_now_playing<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &AppState<C, A>) {
    match state.queue.current() {
        Some(track) => {
            ui.heading(track.display());
            ui.label(format!("by {}", track.artist));
            ui.separator();
            ui.label(format!("requested: {}", state.audio.last_requested().unwrap_or("none")));
        }
        None => {
            debug_assert!(state.queue.is_empty());
            ui.label("Queue is empty. Search and enqueue a track first.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{AudioStub, CatalogStub};

    fn wired() -> AppState<CatalogStub, AudioStub> {
        AppState::new(CatalogStub::new(), AudioStub::new())
    }

    #[test]
    fn empty_query_keeps_page_and_queue() {
        let mut state = wired();
        state.search_text = "   ".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Search);
        assert_eq!(state.queue.len(), 0);
    }

    #[test]
    fn search_hit_enqueues_and_navigates_to_queue() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Queue);
        assert_eq!(state.queue.len(), 1);
    }

    #[test]
    fn play_head_navigates_to_now_playing() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.play_head();
        assert_eq!(state.page, Page::NowPlaying);
    }
}
