//! eframe application wrapper and root view dispatching.

use eframe::egui;

use crate::app::components::{render_player_bar, render_top_bar};
use crate::app::pages::{
    render_detail, render_library, render_now_playing, render_queue, render_search,
};
use crate::app::{AppState, Page};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// eframe application wrapper (thin view over [`AppState`]).
pub struct MoggytApp<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache> {
    state: AppState<C, A, S, L, K>,
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    MoggytApp<C, A, S, L, K>
{
    /// Wraps an already-wired state for eframe.
    pub fn new(state: AppState<C, A, S, L, K>) -> Self {
        Self { state }
    }
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache> eframe::App
    for MoggytApp<C, A, S, L, K>
{
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.state.drain_tray() {
            self.state.save_queue();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.state.sync_presence();
        self.state.continue_autoplay_if_ended();
        self.state.poll_sleep_timer();
        self.state.check_auto_skip();
        if !self.state.text_focused {
            for key in [
                egui::Key::Space,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::N,
                egui::Key::P,
                egui::Key::L,
            ] {
                if ui.input(|i| i.key_pressed(key)) {
                    self.state.apply_key(key);
                    break;
                }
            }
        }
        render_top_bar(ui, &mut self.state);
        render_player_bar(ui, &mut self.state);
        render_body(ui, &mut self.state);
    }
}

fn render_body<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    egui::CentralPanel::default().show(ui, |ui| match state.page {
        Page::Search => render_search(ui, state),
        Page::Queue => render_queue(ui, state),
        Page::NowPlaying => render_now_playing(ui, state),
        Page::Library => render_library(ui, state),
        Page::Detail => render_detail(ui, state),
    });
}
