//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (YouTube Music API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod innertube;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use innertube::{InnerTube, RawTrack, search_body, search_url};
pub use ytm::{Catalog, CatalogStub};
