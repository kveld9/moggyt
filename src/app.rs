//! UI shell: navigation state and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.

use eframe::egui;

use crate::domain::{PlaybackQueue, SearchQuery};
use crate::infra::{AudioBackend, Catalog, SessionStore, sanitize_cookie_input};

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
    /// Library view (playlists, liked tracks, saved albums).
    Library,
}

/// Wiring between the UI shell and its collaborators.
pub struct AppState<C: Catalog, A: AudioBackend, S: SessionStore> {
    /// Active navigation page.
    pub page: Page,
    /// Pure playback queue (no I/O).
    pub queue: PlaybackQueue,
    /// Text typed into the search box (UI state only).
    pub search_text: String,
    /// Text typed into the sign-in cookie box (UI state only).
    pub cookie_text: String,
    /// Catalog adapter (injected port).
    pub catalog: C,
    /// Audio adapter (injected port).
    pub audio: A,
    /// Session store adapter (injected port).
    pub session: S,
}

impl<C: Catalog, A: AudioBackend, S: SessionStore> AppState<C, A, S> {
    /// Creates the shell with injected collaborators.
    pub fn new(catalog: C, audio: A, session: S) -> Self {
        Self {
            page: Page::default(),
            queue: PlaybackQueue::new(),
            search_text: String::new(),
            cookie_text: String::new(),
            catalog,
            audio,
            session,
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

    /// Returns true if an active session is currently stored.
    pub fn is_signed_in(&self) -> bool {
        self.session.load().is_some()
    }

    /// Attempts to parse and save the current cookie input.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn sign_in_from_input(&mut self) -> bool {
        let Some(cleaned) = sanitize_cookie_input(&self.cookie_text) else {
            return false;
        };
        self.session.save(&cleaned);
        self.cookie_text.clear();
        true
    }

    /// Clears the stored session and resets the cookie input buffer.
    pub fn sign_out(&mut self) {
        self.session.clear();
        self.cookie_text.clear();
    }

    /// Returns true if the track id is present in the user's liked collection.
    pub fn is_liked(&self, track_id: &str) -> bool {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return false;
        }
        self.catalog.liked_tracks().iter().any(|track| track.id == trimmed)
    }

    /// Toggles the liked status of a track in the catalog. Blank identifiers are ignored.
    pub fn toggle_like(&mut self, track_id: &str) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let next_liked = !self.is_liked(trimmed);
        self.catalog.set_liked(trimmed, next_liked);
    }
}

/// eframe application wrapper (thin view over [`AppState`]).
pub struct MoggytApp<C: Catalog, A: AudioBackend, S: SessionStore> {
    state: AppState<C, A, S>,
}

impl<C: Catalog, A: AudioBackend, S: SessionStore> MoggytApp<C, A, S> {
    /// Wraps an already-wired state for eframe.
    pub fn new(state: AppState<C, A, S>) -> Self {
        Self { state }
    }
}

impl<C: Catalog, A: AudioBackend, S: SessionStore> eframe::App for MoggytApp<C, A, S> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        render_top_bar(ui, &mut self.state);
        render_player_bar(ui, &mut self.state);
        render_body(ui, &mut self.state);
    }
}

fn render_top_bar<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("moggyt");
            ui.separator();
            ui.selectable_value(&mut state.page, Page::Search, "Search");
            ui.selectable_value(&mut state.page, Page::Queue, "Queue");
            ui.selectable_value(&mut state.page, Page::NowPlaying, "Now playing");
            ui.selectable_value(&mut state.page, Page::Library, "Library");
        });
    });
}

fn render_body<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    egui::CentralPanel::default().show(ui, |ui| match state.page {
        Page::Search => render_search(ui, state),
        Page::Queue => render_queue(ui, state),
        Page::NowPlaying => render_now_playing(ui, state),
        Page::Library => render_library(ui, state),
    });
}

fn render_library<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    render_library_sign_in(ui, state);
    ui.separator();
    render_library_playlists(ui, state);
    ui.separator();
    render_library_liked(ui, state);
    ui.separator();
    render_library_albums(ui, state);
}

fn render_library_sign_in<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    let status = if state.is_signed_in() {
        "Signed in — library unlocked"
    } else {
        "Anonymous — paste cookies to unlock library"
    };
    ui.label(status);
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut state.cookie_text);
        if ui.button("Save").clicked() {
            state.sign_in_from_input();
        }
        if ui.button("Clear").clicked() {
            state.sign_out();
        }
    });
}

fn render_library_playlists<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S>,
) {
    ui.heading("Playlists");
    let playlists = state.catalog.playlists();
    if playlists.is_empty() {
        ui.label("No playlists");
        return;
    }
    for playlist in playlists {
        ui.label(format!("{} ({} tracks)", playlist.title, playlist.track_count));
    }
}

fn render_library_liked<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    ui.heading("Liked songs");
    let liked = state.catalog.liked_tracks();
    if liked.is_empty() {
        ui.label("No liked tracks");
        return;
    }
    let mut to_toggle = None;
    for track in &liked {
        ui.horizontal(|ui| {
            ui.label(track.display());
            if ui.button("Unlike").clicked() {
                to_toggle = Some(track.id.clone());
            }
        });
    }
    if let Some(id) = to_toggle {
        state.toggle_like(&id);
    }
}

fn render_library_albums<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S>,
) {
    ui.heading("Saved albums");
    let albums = state.catalog.saved_albums();
    if albums.is_empty() {
        ui.label("No saved albums");
        return;
    }
    for album in albums {
        ui.label(format!("{} - {}", album.title, album.artist));
    }
}

fn render_search<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    ui.label("Search the catalog (stub returns fixed hits):");
    let submitted = ui.text_edit_singleline(&mut state.search_text).lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if ui.button("Search").clicked() || submitted {
        state.submit_search();
    }
}

fn render_queue<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
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

fn render_now_playing<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S>,
) {
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

fn render_player_bar<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
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

fn render_track_info<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S>,
) {
    let title = state.queue.current().map_or("Queue empty".to_owned(), |t| t.display());
    ui.label(title);
}

fn render_playback_controls<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
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

fn render_seek_controls<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    if ui.button("-10s").clicked() {
        state.seek_by(-10);
    }
    if ui.button("+10s").clicked() {
        state.seek_by(10);
    }
}

fn render_volume_controls<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S>,
) {
    let mut vol = state.audio.volume();
    if ui.add(egui::Slider::new(&mut vol, 0..=100).text("Vol")).changed() {
        state.set_volume(vol);
    }
}

fn render_position_info<C: Catalog, A: AudioBackend, S: SessionStore>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S>,
) {
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
    use crate::infra::{AudioStub, CatalogStub, MemorySessionStore};

    fn wired() -> AppState<CatalogStub, AudioStub, MemorySessionStore> {
        AppState::new(CatalogStub::new(), AudioStub::new(), MemorySessionStore::new())
    }

    #[test]
    fn sign_in_and_out_lifecycle_with_sanitized_input() {
        let mut state = wired();
        assert!(!state.is_signed_in());

        state.cookie_text = "  cookie: SID=secret123; HSID=secret456  ".to_owned();
        let ok = state.sign_in_from_input();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.cookie_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=secret123; HSID=secret456"));

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn sign_in_from_input_rejects_empty_or_whitespace() {
        let mut state = wired();
        for invalid in ["", "   ", "cookie:", "COOKIE:   "] {
            state.cookie_text = invalid.to_owned();
            let ok = state.sign_in_from_input();
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert!(!state.is_signed_in());
        }
    }

    #[test]
    fn toggle_like_flips_state_and_ignores_blank() {
        let mut state = wired();

        // Stub catalog defaults to "stub-1" liked.
        assert!(state.is_liked("stub-1"));
        assert!(!state.is_liked("stub-2"));

        // Toggle existing liked track off.
        state.toggle_like("stub-1");
        assert!(!state.is_liked("stub-1"));

        // Toggle back on.
        state.toggle_like("stub-1");
        assert!(state.is_liked("stub-1"));

        // Toggle unliked track on.
        state.toggle_like("stub-2");
        assert!(state.is_liked("stub-2"));

        // Blank and whitespace ids are ignored.
        state.toggle_like("");
        state.toggle_like("   ");
        assert!(state.is_liked("stub-1"));
        assert!(state.is_liked("stub-2"));
        assert!(!state.is_liked(""));
        assert!(!state.is_liked("   "));
    }

    #[test]
    fn library_page_selection_and_state() {
        let mut state = wired();
        assert_eq!(state.page, Page::Search);
        state.page = Page::Library;
        assert_eq!(state.page, Page::Library);
        assert!(!state.catalog.playlists().is_empty());
        assert!(!state.catalog.liked_tracks().is_empty());
        assert!(!state.catalog.saved_albums().is_empty());
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
