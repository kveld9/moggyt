//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (YouTube Music API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod cover;
mod lyrics;
mod presence;
mod session;
mod stream;
mod updater;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use lyrics::{LrclibStub, LyricLine, LyricsSource, SyncedLyrics};
pub use presence::{NoopPresence, Presence};
pub use session::{MemorySessionStore, SessionStore};
pub use stream::{StreamResolver, StreamStub};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use ytm::{Catalog, CatalogStub};
