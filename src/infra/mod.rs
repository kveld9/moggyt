//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (YouTube Music API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod cover;
mod innertube;
mod lyrics;
mod presence;
mod session;
mod stream;
mod updater;
mod ytdlp;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use innertube::{InnerTube, RawTrack, search_body, search_url};
pub use lyrics::{LrclibStub, LyricLine, LyricsSource, SyncedLyrics};
pub use presence::{NoopPresence, Presence};
pub use session::{MemorySessionStore, SessionStore};
pub use stream::{StreamResolver, StreamStub};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, ensure_sidecar, format_args,
    parse_version, release_asset_name, sidecar_needs_update, watch_url,
};
pub use ytm::{Catalog, CatalogStub};
