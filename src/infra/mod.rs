//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (YouTube Music API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod audio_rodio;
mod cover;
mod cover_disk;
mod innertube;
mod instance;
mod lyrics;
mod lyrics_lrclib;
mod presence;
mod session;
mod session_keyring;
mod stream;
mod tray;
mod updater;
mod ytdlp;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use audio_rodio::{LiveAudio, RodioBackend, volume_to_amplitude};
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use cover_disk::{DiskCoverCache, cache_file_name, fnv1a_hex};
pub use innertube::{InnerTube, RawTrack, search_body, search_url};
pub use instance::{SingleInstanceGuard, instance_lock_name};
pub use lyrics::{LrclibStub, LyricLine, LyricsSource, SyncedLyrics};
pub use lyrics_lrclib::{LrclibClient, lrclib_get_url, parse_lrc};
pub use presence::{NoopPresence, Presence};
pub use session::{MemorySessionStore, SessionStore};
pub use session_keyring::{KeyringSessionStore, sanitize_cookie_input};
pub use stream::{StreamResolver, StreamStub};
pub use tray::{TrayAction, TrayHandle};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, ensure_sidecar, format_args,
    parse_version, release_asset_name, sidecar_needs_update, watch_url,
};
pub use ytm::{Catalog, CatalogStub, YtmCatalog};
