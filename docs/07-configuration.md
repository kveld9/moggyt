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
  transmit account cookies. Account credentials are used strictly for catalog library endpoints.

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

Persistent user preferences are saved in `settings.json` inside the standard configuration directory:

| Platform | Configuration Directory | Fallback |
| --- | --- | --- |
| Linux / Unix | `$XDG_CONFIG_HOME/moggyt` | `~/.config/moggyt` (or `.moggyt` relative directory) |
| Windows | `%APPDATA%\moggyt` | `.moggyt` relative directory |

### Settings File Format

The configuration file uses JSON serialization with atomic replacement:

```json
{
  "theme": "white",
  "locale": "en",
  "volume": 100,
  "quality": "standard",
  "keyboard_shortcuts": true
}
```

### Supported Options

- **Themes (`theme`)**:
  - `"white"`: Clean light palette (tranche 1 default).
  - `"black-oled"`: Pure black background (`#000000`) designed for OLED panels.
  - `"mono"`: High-contrast monochromatic palette with zero color saturation across all slots.
- **Locales (`locale`)**:
  - `"en"`: English (fallback when system language is not Spanish or unparseable).
  - `"es"`: Spanish (`Español`).
  - First-run resolution detects the system locale via environment variables (`LC_ALL`, `LC_MESSAGES`, `LANG`),
    falling back to English; explicitly saved choices in `settings.json` take precedence.
- **Volume (`volume`)**:
  - Integer audio level between `0` and `100` (default `100`). Clamped to `0..=100` on load.
- **Audio Quality (`quality`)**:
  - `"compact"`: Low-bandwidth audio stream using yt-dlp selector `"bestaudio[abr<=64]/bestaudio/best"`.
  - `"standard"`: Balanced audio stream using yt-dlp selector `"bestaudio/best"` (default).
  - `"high"`: High-fidelity audio stream using yt-dlp selector `"bestaudio[abr>=192]/bestaudio/best"`.
- **Keyboard Shortcuts (`keyboard_shortcuts`)**:
  - `true`: Global transport and playback shortcuts enabled (default).
  - `false`: Shortcuts disabled.

### Fault Tolerance & Atomic Persistence

Settings are saved by writing to a temporary file in the configuration directory followed by an atomic rename, ensuring
that unexpected termination never leaves a corrupt or truncated configuration file. If `settings.json` is missing,
empty, or corrupted, `moggyt` automatically falls back to default settings without crashing.

## External Integrations

- **Discord Rich Presence**: stays dormant in this build because `DISCORD_APP_ID` is 0 (unregistered); no IPC
  connection is attempted and playback is unaffected. Setting a valid Discord application client ID is required to
  activate it.
- **Last.fm Scrobbler**: initialized with dormant stub credentials by default. Scrobbles are submitted only after
  credentials and API keys are registered.
