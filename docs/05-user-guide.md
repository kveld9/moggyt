# User Guide

This guide covers the interface layout, playback controls, queue management, and library features in `moggyt`.

## Interface Layout

The interface consists of three primary regions:

1. **Top Bar**: displays application title, page navigation tabs (`Search`, `Queue`, `Now playing`, `Library`), and an
   update badge when a newer release is detected.
2. **Body Panel**: renders the active page selected in the top bar.
3. **Player Bar**: persistent bottom panel providing transport controls, volume slider, and track position.

## Navigation Pages

### Search Page

The search view presents curated home shelves and accepts queries for tracks, artists, and albums:

- **Home Shelves**: displays categorized carousels (albums, artists, playlists) loaded lazily on open. Click `Refresh`
  to clear the cache and refetch home shelves from the catalog.
- Type your search terms into the single-line input field.
- Press `Enter` or click the `Search` button.
- Catalog hits are queried through the catalog adapter and the top hit is enqueued automatically, transitioning the
  view to the `Queue` page.

### Queue Page

The queue view manages the ordered list of upcoming tracks:

- Displays the total number of queued tracks (`N track(s) queued`).
- `Previous`: retreats the playback cursor to the preceding track in the queue.
- `Play head`: starts playback of the track currently at the cursor and switches navigation to `Now playing`.
- `Next`: advances the playback cursor to the subsequent track in the queue.
- Below the controls, every queued track is listed in order (`title - artist`).

### Now Playing Page

The now playing view presents rich playback details for the track under the active cursor:

- Displays track title, artist name, and the stream identifier requested from the audio backend.
- **Start Radio**: click `Start radio` to seed an autoplay continuation queue with the active track
  and automatically fetch recommendations when reaching the end of the queue.
- **Cover Art**: displays artwork fetched from the track thumbnail URL and cached locally. If offline or pending,
  shows `Cover art unavailable (offline or pending fetch)`.
- **Synced Lyrics**: displays timestamped lyrics fetched from LRCLIB for the active artist and title pair.
  - Offset controls: click `-250ms` or `+250ms` to adjust lyric synchronization dynamically.
  - Formatted timestamps (`m:ss`) precede each line of text.
  - Shows `No synced lyrics for this track.` when no matching lyrics exist on LRCLIB.
- If the queue is empty, displays `Queue is empty. Search and enqueue a track first.`.

### Library Page

The library view manages account authentication, release update actions, and saved collection views:

- **Authentication / Sign-in**:
  - Displays authentication status: `Signed in — library unlocked` or `Anonymous — paste cookies to unlock library`.
  - Input field: paste raw cookie string from your browser session on `music.youtube.com`.
  - `Save`: sanitizes input, stores session cookies securely in the OS credential store (keyring), and unlocks library.
  - `Clear`: removes stored credentials from the keyring and signs out.
- **Updates Section**:
  - Displays current update status (`Never checked`, `moggyt is up to date`, `Update available: <version>`, or
    `Update check failed (offline?)`).
  - `Check for updates`: triggers an asynchronous query to GitHub Releases.
  - When an update is available, displays a direct link to the GitHub download page and a `Dismiss` button to clear the
    notice for the session.
- **Playlists**: lists saved playlists with title and track counts. Shows `No playlists` when empty.
- **Liked Songs**: lists liked tracks. Each row provides track details and an `Unlike` button.
- **Saved Albums**: lists saved albums by title and artist. Shows `No saved albums` when empty.

## Persistent Player Bar

The bottom player bar remains visible across all pages and provides continuous transport controls:

- **Track Info**: displays `title - artist` of the track under the cursor, or `Queue empty` when empty.
- **Playback Controls**:
  - `Previous`: skips to previous track in the queue and starts playback.
  - `Play` / `Pause`: toggles between playing and paused audio states.
  - `Next`: skips to next track in the queue and starts playback.
- **Seek Controls**:
  - `-10s`: rewinds current playback position by 10 seconds (clamped at 0).
  - `+10s`: advances playback position forward by 10 seconds.
- **Volume Slider**: interactive slider setting audio volume from 0 to 100.
- **Position Display**: shows elapsed seconds against total duration (`<elapsed>s / <duration>s` or `<elapsed>s`).

## Desktop Integration

### System Tray

When supported by the desktop environment, `moggyt` creates a system tray icon:

- Supports background transport actions: toggle playback, skip next, skip previous.
- Context menu option to quit the application cleanly.
- If the desktop environment lacks system tray support, the application logs a warning and runs normally without an
  icon.

### MPRIS and SMTC

- **Linux (MPRIS)**: exports active playback metadata (track title, artist, play/pause state) over D-Bus to system
  media widgets and desktop shells.
- **Windows (SMTC)**: synchronizes media metadata with Windows System Media Transport Controls.
- **Operational Note**: in v1, desktop media presence operates as outbound metadata synchronization only; inbound media
  key routing from system shell buttons is deferred.

### Discord Presence & Last.fm Scrobbling

- **Discord Rich Presence**: dormant in this build (`DISCORD_APP_ID` is 0, no IPC attempted); it will publish track
  title and artist only once a valid application ID is configured.
- **Last.fm Scrobbler**: initialized in a dormant state by default until credentials and registration are configured.
