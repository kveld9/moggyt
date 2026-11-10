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

## Audio Stream Disk Cache

Decoded and downloaded audio streams are cached on local storage to minimize redundant network bandwidth and bypass
stream resolution for repeated plays:

- **Cache Path**: `<cache_dir>/streams` (`$XDG_CACHE_HOME/moggyt/streams` on Linux, `%LOCALAPPDATA%\moggyt\streams` on
  Windows; falling back to `<data_dir>/streams`).
- **Naming Scheme**: cache filenames are derived by hashing video/track IDs using the FNV-1a 64-bit algorithm
  (`<hash>.stream`, 16-char lowercase hex).
- **Eviction Policy**: least-recently-used (LRU) based on file access timestamp (`mtime`). When the total size of cached
  `.stream` files exceeds `stream_cache_limit_mb`, the oldest accessed files are pruned until cache usage falls within
  the configured byte budget.
- **Atomic Writes**: streams are written into temporary `.tmp.*` files and renamed atomically upon completion.

## Play Log

Local listening history is recorded to an append-only JSON Lines file in the application data directory:

- **Path**: `<data_dir>/play-log.jsonl`
- **Format**: JSON Lines (`.jsonl`), where each completed playback entry is written as a single line containing one
  JSON object.
- **Fields**:
  - `video_id` (string): Stable catalog identifier or video id.
  - `title` (string): Track title.
  - `artist` (string): Artist display name.
  - `album` (optional string): Album name when known; omitted when absent.
  - `started_at_unix` (integer): Unix epoch timestamp in seconds when playback started.
  - `listened_secs` (integer): Wall-clock time in seconds spent listening, clamped to track duration when known.
  - `duration_secs` (optional integer): Track duration in seconds when known; omitted when absent.
- **Recording Threshold**: a play is recorded when playback ends (via next track, previous track, queue/autoplay
  advance, or starting another track) and elapsed listening time is at least 30 seconds (`MIN_RECORDED_PLAY_SECS`).
- **Timing & Clamping**: listened time is measured from the moment the track starts and is clamped to the track's
  catalog duration when known (pauses currently count toward elapsed wall-clock time).
- **Exit Behavior**: any play in progress when the application exits is not recorded.
- **Fault Tolerance**: when loading playback records, malformed or truncated lines (such as from a crash or power cut)
  are skipped with a warning log without failing the remaining history load.

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
RUST_LOG=moggyt::infra=debug,moggyt::slint_shell=trace cargo run
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
  "custom_theme_name": null,
  "locale": "en",
  "volume": 100,
  "crossfade_seconds": 0,
  "stream_cache_limit_mb": 512,
  "quality": "standard",
  "songs_only": false,
  "sponsorblock_enabled": true,
  "discord_presence_enabled": false,
  "discord_app_id": null,
  "lastfm_enabled": false,
  "lastfm_username": "",
  "lastfm_api_key": "",
  "lastfm_api_secret": "",
  "player_bar_mode": "classic",
  "player_bar_show_artwork": true,
  "player_bar_show_like": true,
  "player_bar_show_shuffle": true,
  "player_bar_show_repeat": true,
  "player_bar_show_skip_seconds": false,
  "player_bar_show_queue": true,
  "player_bar_show_volume": true
}
```

### Supported Options

- **Themes (`theme`)**:
  - `"white"`: Clean light palette (tranche 1 default).
  - `"black-oled"`: Pure black background (`#000000`) designed for OLED panels.
  - `"mono"`: High-contrast monochromatic palette with zero color saturation across all slots.
  - `"custom"`: Palette loaded from the active custom theme JSON file, falling back to Black OLED until a valid
    file loads.
- **Custom Theme Name (`custom_theme_name`)**:
  - Optional string (default `null`). Identifies the selected custom theme name corresponding to a
    `<config_dir>/themes/<name>.json` file or `"custom-theme"` for `<config_dir>/custom-theme.json`.
- **Locales (`locale`)**:
  - `"en"`: English (fallback when system language is not Spanish or unparseable).
  - `"es"`: Spanish (`Español`).
  - First-run resolution detects the system locale via environment variables (`LC_ALL`, `LC_MESSAGES`, `LANG`),
    falling back to English; explicitly saved choices in `settings.json` take precedence.
- **Volume (`volume`)**:
  - Integer audio level between `0` and `100` (default `100`). Clamped to `0..=100` on load. The slider maps to
    amplitude through a cubic perceptual curve (`(volume / 100)^3`), where 50 percent equals -18 dB and 100 is unity
    gain.
- **Track Crossfade (`crossfade_seconds`)**:
  - Integer crossfade transition duration in seconds between `0` and `15` (default `0` = disabled). Clamped to
    `0..=15` on load. When enabled, smoothly crossfades between tracks using dual-sink volume ramps.
  - In Settings it is adjusted with an interactive slider from `0` to `12` seconds; `0` disables crossfade.
- **Stream Cache Quota (`stream_cache_limit_mb`)**:
  - Integer maximum disk budget in megabytes for the audio stream cache (default `512`). Clamped to `0..=10240` (up to
    10 GB) on load. Setting to `0` disables stream caching on disk.
- **Audio Quality (`quality`)**:
  - Selectable in Settings > Playback and applies from the next track.
  - `"compact"`: Low-bandwidth audio stream using yt-dlp selector
    `"bestaudio[ext=m4a][abr<=64]/bestaudio[ext=m4a]/bestaudio/best"`.
  - `"standard"`: Balanced audio stream using yt-dlp selector `"bestaudio[ext=m4a]/bestaudio/best"` (default).
  - `"high"`: High-fidelity audio stream using yt-dlp selector
    `"bestaudio[ext=m4a][abr>=192]/bestaudio[ext=m4a]/bestaudio/best"`.
- **Prefer Songs Over Videos (`songs_only`)**:
  - `false`: Search results and autoplay radio retain both official tracks and music videos (default).
  - `true`: Search results and autoplay radio drop music videos (InnerTube `OMV` and `UGC` types), keeping audio tracks
    (`ATV`) and unknown types. If dropping music videos would remove every track from a result list, the fallback
    preserves the original list unfiltered so playback and autoplay do not stall.
- **Community Skip Segments (`sponsorblock_enabled`)**:
  - `sponsorblock_enabled` (boolean, default `true`):
    - `true`: Community SponsorBlock segments are fetched once per track and skipped automatically during playback.
      Only the first 4 hexadecimal characters of the SHA-256 digest of the video id are sent (k-anonymity).
      Locally persisted ranges stored in `skips.json` always take precedence over community ranges.
    - `false`: Community skip segment fetching and automatic skipping are disabled.
- **Discord Rich Presence (`discord_presence_enabled`)**:
  - `discord_presence_enabled` (boolean, default `false`):
    - `false`: Discord Rich Presence disabled; no IPC connection is attempted (default).
    - `true`: Outbound Rich Presence enabled; broadcasts a "Listening to moggyt" activity with track title and artist.
- **Last.fm Integration (`lastfm_enabled`, `lastfm_username`, `lastfm_api_key`, `lastfm_api_secret`)**:
  - Application keys are provided at compile time (release builds receive them from repository secrets; local builds may
    set `MOGGYT_LASTFM_API_KEY` and `MOGGYT_LASTFM_SECRET`). The settings panel exposes only Connect and Disconnect
    actions.
  - `lastfm_enabled` (boolean, default `false`): Whether live scrobbling to Last.fm is enabled.
  - `lastfm_username` (string, default `""`): Display name of the connected Last.fm account.
  - `lastfm_api_key` (string, default `""`): Optional manual override in `settings.json` taking precedence over the
    compile-time environment variable (`MOGGYT_LASTFM_API_KEY`).
  - `lastfm_api_secret` (string, default `""`): Optional manual override in `settings.json` taking precedence over the
    compile-time environment variable (`MOGGYT_LASTFM_SECRET`).
- **Player Bar Mode (`player_bar_mode`)**:
  - `"classic"`: Docked full-width bar flush with window bottom edge (default).
  - `"floating"`: Rounded, centered card floating above the bottom edge capped at 960px.
- **Player Bar Elements**:
  - `player_bar_show_artwork` (boolean, default `true`): Whether cover artwork thumbnail is shown.
  - `player_bar_show_like` (boolean, default `true`): Whether like/favorite heart button is shown.
  - `player_bar_show_shuffle` (boolean, default `true`): Whether shuffle button is shown.
  - `player_bar_show_repeat` (boolean, default `true`): Whether repeat cycle button is shown.
  - `player_bar_show_skip_seconds` (boolean, default `false`): Whether `-10s` / `+10s` buttons are shown.
  - `player_bar_show_queue` (boolean, default `true`): Whether queue button is shown.
  - `player_bar_show_volume` (boolean, default `true`): Whether speaker icon and volume slider are shown.

Note: Keyboard shortcuts are always on and have no configuration key.

### Custom Theme Files and Themes Directory

User-defined color palettes can be loaded from individual theme files inside `<config_dir>/themes/*.json` or from
`custom-theme.json` in the configuration directory:

- **Themes Directory**: `<config_dir>/themes/` is scanned for `.json` files. The file stem becomes the theme identifier
  (for example, `themes/rose-pine.json` becomes `rose-pine`).
- **Legacy Fallback**: `<config_dir>/custom-theme.json` is automatically included as `"custom-theme"` when present.
- **In-App Selection**: when Custom theme is active in Settings > Theme, an interactive selector displays all available
  custom themes. Selecting a theme immediately activates it and persists `custom_theme_name` to `settings.json`. If no
  custom themes are installed, an informational hint directs users to place `.json` theme files in `themes/`.

```json
{
  "mode": "dark",
  "window": "#121318",
  "panel": "#1e1f25",
  "surface": "#282a2f",
  "text": "#e2e2e9",
  "secondary": "#c5c6d0",
  "accent": "#b1c5ff",
  "border": "#44464f",
  "hover": "#33343a",
  "overlay": "#38393f"
}
```

#### Inheritance and Strictness Rules

- `"mode"` is `"dark"` or `"light"` (required).
- The nine slot keys match `Palette` fields (`window`, `panel`, `surface`, `text`, `secondary`, `accent`, `border`,
  `hover`, `overlay`).
- Each slot value must be formatted as `"#rrggbb"` (case-insensitive 6-digit hex with leading `#`).
- Missing slots inherit from a base palette: `Palette::black_oled()` when dark, `Palette::white()` when light.
- Any unrecognized key causes a validation error for typo safety.
- While the Custom theme is active, the application polls and reloads the active theme file within about half a second
  of a change.
- If a write or parse error occurs (such as a half-written file), the client logs a warning, keeps the last valid
  palette, and displays the error in the theme settings panel.

#### Matugen Integration

A Material You palette generated by `matugen` can drive app colors via the shipped template:

```toml
# ~/.config/matugen/config.toml
[templates.moggyt]
input_path = '/path/to/moggyt/packaging/matugen/moggyt-theme.json'
output_path = '~/.config/moggyt/custom-theme.json'
```

Generate colors from wallpaper or hex:

```bash
matugen image <wallpaper>
# or
matugen color hex '#3366cc'
```

The template maps Material You color roles to moggyt slots as follows:

- `window` = `surface`
- `panel` = `surface_container`
- `surface` = `surface_container_high`
- `text` = `on_surface`
- `secondary` = `on_surface_variant`
- `accent` = `primary`
- `border` = `outline_variant`
- `hover` = `surface_container_highest`
- `overlay` = `surface_bright`

### Fault Tolerance & Atomic Persistence

Settings are saved by writing to a temporary file in the configuration directory followed by an atomic rename, ensuring
that unexpected termination never leaves a corrupt or truncated configuration file. If `settings.json` is missing,
empty, or corrupted, `moggyt` automatically falls back to default settings without crashing.

## External Integrations

- **Discord Rich Presence**: opt-in toggle in Settings > Integrations. Broadcasts a "Listening to moggyt" activity with
  title and artist; connects lazily when Discord is running (also if Discord starts later, on the next track change).
  Disabling clears the status.
- **Last.fm Scrobbler**: application keys are injected at compile time from `MOGGYT_LASTFM_API_KEY` and
  `MOGGYT_LASTFM_SECRET` (release builds get them from repository secrets; local builds without them keep
  Last.fm dormant; for local development export them before `cargo build`, e.g.
  `set -a; . ~/.config/moggyt/lastfm-dev.env; set +a`); the per-user session key lives in the OS keyring; requests
  are sent off the UI thread with a 10 s timeout; connect via Settings > Integrations > Connect with Last.fm, which
  opens the browser authorization flow; authorize within 3 minutes; Disconnect removes the session from the keyring.
