//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (catalog API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod audio_rodio;
mod catalog_client;
mod cover;
mod cover_art;
mod cover_disk;
mod instance;
mod lyrics;
mod lyrics_lrclib;
mod presence;
mod presence_desktop;
mod presence_discord;
mod scrobble;
mod session;
mod session_keyring;
mod settings;
mod stream;
mod tray;
mod updater;
mod updater_github;
mod ytdlp;
mod ytm;

pub use audio::{AudioBackend, AudioStub};
pub use audio_rodio::{LiveAudio, RodioBackend, volume_to_amplitude};
pub use catalog_client::{CatalogClient, RawTrack, search_body, search_url};
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use cover_art::{COVER_HIRES_MAX_PX, COVER_MAX_PX, decode_cover, fetch_artwork};
pub use cover_disk::{DiskCoverCache, cache_file_name, fnv1a_hex};
pub use instance::{SingleInstanceGuard, instance_lock_name};
pub use lyrics::{LrclibStub, LyricLine, LyricsSource, SyncedLyrics};
pub use lyrics_lrclib::{LrclibClient, lrclib_get_url, parse_lrc};
pub use presence::{NoopPresence, Presence};
pub use presence_desktop::{DesktopPresence, MediaKeyAction, action_for, metadata_for, status_for};
pub use presence_discord::{DISCORD_APP_ID, DiscordPresence, activity_parts};
pub use scrobble::{
    CredentialStore, KeyringCredentialStore, LastFmClient, LastFmCredentials,
    MemoryCredentialStore, Scrobbler, StubScrobbler, api_signature, client_from_credentials,
    should_scrobble,
};
pub use session::{MemorySessionStore, SessionStore};
pub use session_keyring::{KeyringSessionStore, sanitize_cookie_input};
pub use settings::{
    DiskSettingsStore, MemorySettingsStore, Settings, SettingsStore, default_config_dir,
};
pub use stream::{AudioQuality, StreamResolver, StreamStub};
pub use tray::{TrayAction, TrayHandle};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use updater_github::{
    GithubUpdater, UPDATE_OWNER_REPO, is_newer, latest_url, parse_release_payload,
};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, ensure_sidecar, format_args,
    parse_version, release_asset_name, sidecar_needs_update, watch_url,
};
pub use ytm::{Catalog, CatalogStub, LiveCatalog};
