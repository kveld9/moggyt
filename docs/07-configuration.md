# Configuration

This document outlines storage locations, runtime options, credential management, and caching behavior in `moggyt`.

## Data Directories

Application data, including the `yt-dlp` stream resolution sidecar and disk caches, resides in standard OS paths:

| Platform | Location | Fallback |
| --- | --- | --- |
| Linux / Unix | `$XDG_DATA_HOME/moggyt` | `~/.local/share/moggyt` (or `.moggyt` relative directory) |
| Windows | `%APPDATA%\moggyt` | `.moggyt` relative directory |

The application automatically creates this directory structure during startup if it does not already exist.

## Cover Art Disk Cache

Artwork thumbnails displayed in the `Now Playing` page are cached on local storage:

- **Primary Path**: `<data_dir>/covers`
- **Fallback Path**: if creating or writing to the primary cache directory fails, the client logs a warning and
  redirects cache writes to the operating system temporary directory (`/tmp/moggyt-covers-fallback` on Linux or
  `%TEMP%\moggyt-covers-fallback` on Windows). If fallback initialization also fails, the process exits with a fatal
  error.
- **Naming Scheme**: cache filenames are derived by hashing artwork URLs using the FNV-1a 64-bit algorithm
  (`<hash>.cover`, 16-char lowercase hex).
- **Memory LRU**: an in-memory cache layer retains recent textures to prevent redundant disk reads and UI thread
  stalls.

## Authentication and Credential Storage

User authentication for library access (playlists, liked tracks, saved albums) relies on importing session cookies from
an active `music.youtube.com` browser session:

- **Keyring Storage**: imported cookies are stored exclusively in the operating system credential store using the
  system keyring provider:
  - Linux: Secret Service API via D-Bus (e.g. GNOME Keyring, KWallet, or KeePassXC).
  - Windows: Windows Credential Manager.
- **Zero Disk Exposure**: raw session cookies and access tokens are never written to plain text configuration files,
  caches, or log outputs.
- **Anonymous Streaming Invariant**: audio streaming requests and `yt-dlp` sidecar queries remain anonymous and never
  transmit account cookies. Account credentials are used strictly for InnerTube library endpoints.

## Logging and Diagnostics

Application logs are structured using `tracing` and can be controlled at launch via the `RUST_LOG` environment
variable:

```bash
# Default behavior (info level)
cargo run

# Verbose debugging output across all modules
RUST_LOG=debug cargo run

# Target specific modules
RUST_LOG=moggyt::infra=debug,moggyt::app=trace cargo run
```

Log messages record sidecar downloads, audio backend status, cover cache resolutions, and update checks.

## User Preferences and Settings

Per SDD requirement FR-10, persistent user preferences (playback volume, UI theme, streaming quality preferences, and
keyboard shortcuts) are defined for local persistence.

## External Integrations

- **Discord Rich Presence**: stays dormant in this build because `DISCORD_APP_ID` is 0 (unregistered); no IPC
  connection is attempted and playback is unaffected. Setting a valid Discord application client ID is required to
  activate it.
- **Last.fm Scrobbler**: initialized with dormant stub credentials by default. Scrobbles are submitted only after
  credentials and API keys are registered.
