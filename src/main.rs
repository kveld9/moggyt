//! moggyt: native YouTube Music client (eframe/egui).
//!
//! Composition root: builds logging, instantiates the catalog/audio
//! adapters, injects them into the UI shell, and bootstraps eframe.
//! Domain logic lives in [`moggyt::domain`]; all I/O hides behind
//! ports implemented in [`moggyt::infra`].

use moggyt::app::{AppState, MoggytApp};
use moggyt::infra::{AudioStub, CatalogStub};

fn main() -> anyhow::Result<()> {
    tracing_subscriber_setup();

    let catalog = CatalogStub::new();
    let audio = AudioStub::new();
    let state = AppState::new(catalog, audio);

    let options = eframe::NativeOptions::default();
    eframe::run_native("moggyt", options, Box::new(|_ctx| Ok(Box::new(MoggytApp::new(state)))))
        .map_err(|err| anyhow::anyhow!("eframe failed: {err}"))?;

    Ok(())
}

fn tracing_subscriber_setup() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).try_init();
}
