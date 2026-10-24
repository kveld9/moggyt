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

    /// Plays the track currently under the queue cursor without modifying active page.
    pub fn play_current_quiet(&mut self) {
        if let Some(track) = self.queue.current() {
            let id = track.id.clone();
            self.audio.play(&id);
        }
    }

    /// Toggles playback: pauses if currently playing, resumes if previously requested,
    /// or starts playback of current head quietly.
    pub fn toggle_play(&mut self) {
        if self.audio.is_playing() {
            self.audio.pause();
        } else if self.audio.last_requested().is_some() {
            self.audio.resume_play();
        } else {
            self.play_current_quiet();
        }
    }

    /// Advances the queue cursor and deterministically starts playback of the new head.
    pub fn next_track(&mut self) {
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Retreats the queue cursor and deterministically starts playback of the new head.
    pub fn prev_track(&mut self) {
        self.queue.retreat();
        self.play_current_quiet();
    }

    /// Adjusts playback position by `delta_secs` relative to current position, clamped at zero.
    pub fn seek_by(&mut self, delta_secs: i64) {
        let current = self.audio.position_secs() as i64;
        let target = current.saturating_add(delta_secs).max(0) as u64;
        self.audio.seek(target);
    }

    /// Sets playback volume level (0..=100, clamped by audio adapter).
    pub fn set_volume(&mut self, volume: u8) {
        self.audio.set_volume(volume);
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
        render_player_bar(ui, &mut self.state);
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

fn render_player_bar<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &mut AppState<C, A>) {
    egui::Panel::bottom("player_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            render_track_info(ui, state);
            ui.separator();
            render_playback_controls(ui, state);
            ui.separator();
            render_seek_controls(ui, state);
            ui.separator();
            render_volume_controls(ui, state);
            ui.separator();
            render_position_info(ui, state);
        });
    });
}

fn render_track_info<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &AppState<C, A>) {
    let title = state.queue.current().map_or("Queue empty".to_owned(), |t| t.display());
    ui.label(title);
}

fn render_playback_controls<C: Catalog, A: AudioBackend>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A>,
) {
    if ui.button("Previous").clicked() {
        state.prev_track();
    }
    let toggle_label = if state.audio.is_playing() { "Pause" } else { "Play" };
    if ui.button(toggle_label).clicked() {
        state.toggle_play();
    }
    if ui.button("Next").clicked() {
        state.next_track();
    }
}

fn render_seek_controls<C: Catalog, A: AudioBackend>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A>,
) {
    if ui.button("-10s").clicked() {
        state.seek_by(-10);
    }
    if ui.button("+10s").clicked() {
        state.seek_by(10);
    }
}

fn render_volume_controls<C: Catalog, A: AudioBackend>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A>,
) {
    let mut vol = state.audio.volume();
    if ui.add(egui::Slider::new(&mut vol, 0..=100).text("Vol")).changed() {
        state.set_volume(vol);
    }
}

fn render_position_info<C: Catalog, A: AudioBackend>(ui: &mut egui::Ui, state: &AppState<C, A>) {
    let pos = state.audio.position_secs();
    let text = match state.queue.current().and_then(|t| t.duration_secs) {
        Some(dur) => format!("{pos}s / {dur}s"),
        None => format!("{pos}s"),
    };
    ui.label(text);
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

    #[test]
    fn toggle_on_empty_queue_does_nothing() {
        let mut state = wired();
        state.toggle_play();
        assert!(!state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), None);
    }

    #[test]
    fn search_then_toggle_plays_head() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.toggle_play();
        assert!(state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("stub-1"));
        assert_eq!(state.page, Page::Queue);
    }

    #[test]
    fn next_and_prev_move_current_and_play() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.search_text = "sunrise".to_owned();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        state.next_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        state.prev_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
    }

    #[test]
    fn seek_by_clamps_at_zero_and_advances() {
        let mut state = wired();
        state.seek_by(-10);
        assert_eq!(state.audio.position_secs(), 0);

        state.seek_by(15);
        assert_eq!(state.audio.position_secs(), 15);

        state.seek_by(-5);
        assert_eq!(state.audio.position_secs(), 10);

        state.seek_by(-20);
        assert_eq!(state.audio.position_secs(), 0);
    }

    #[test]
    fn set_volume_clamps_via_stub() {
        let mut state = wired();
        state.set_volume(50);
        assert_eq!(state.audio.volume(), 50);

        state.set_volume(150);
        assert_eq!(state.audio.volume(), 100);
    }
}
