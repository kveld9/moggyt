//! UI preview harness for moggyt using in-memory fake adapters.
//! Boots the full application interface without network, audio, or keyring access.
//! Run with: cargo run --example ui_preview

use moggyt::app::{AppState, MoggytApp};
use moggyt::domain::Track;
use moggyt::infra::{
    AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore, StaticUpdater,
};

fn main() -> eframe::Result<()> {
    let mut state = AppState::new(
        CatalogStub::new(),
        AudioStub::new(),
        MemorySessionStore::new(),
        LrclibStub::new(),
        MemoryCoverCache::new(),
    );
    state.check_with(&StaticUpdater::new());
    seed_preview_queue(&mut state);

    let options = eframe::NativeOptions::default();
    let palette = state.theme.palette();
    eframe::run_native(
        "moggyt (UI Preview)",
        options,
        Box::new(move |cc| {
            moggyt::app::theme::apply(&cc.egui_ctx, &palette);
            Ok(Box::new(MoggytApp::new(state)))
        }),
    )
}

fn seed_preview_queue(
    state: &mut AppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache>,
) {
    state.queue.push(
        Track::new("stub-1", "Midnight City", "M83")
            .with_album("Hurry Up, We're Dreaming")
            .with_duration(243),
    );
    state.queue.push(
        Track::new("stub-2", "Starboy", "The Weeknd").with_album("Starboy").with_duration(230),
    );
    state.queue.push(
        Track::new("stub-3", "Get Lucky", "Daft Punk")
            .with_album("Random Access Memories")
            .with_duration(248),
    );
    state.play_current_quiet();
}
