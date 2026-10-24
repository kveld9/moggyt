//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (YouTube Music API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod innertube;
mod stream;
mod ytdlp;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use innertube::{InnerTube, RawTrack, search_body, search_url};
pub use stream::{StreamResolver, StreamStub};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, ensure_sidecar, format_args,
    parse_version, release_asset_name, sidecar_needs_update, watch_url,
};
pub use ytm::{Catalog, CatalogStub};
