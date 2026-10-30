# User Guide

This guide covers the interface layout, playback controls, queue management, and library features in `moggyt`.

## Interface Layout

The interface consists of three primary regions:

1. **Top Bar**: displays application title, page navigation tabs (`Search`, `Now playing`, `Library`, `Detail`),
   and an update badge when a newer release is detected.
2. **Body Panel**: renders the active page selected in the top bar.
3. **Player Bar**: persistent bottom panel providing transport controls, volume slider, and track position.

## Navigation Pages

### Search Page

The search view accepts queries for tracks, artists, and albums, and maintains a persistent recent-search history:

- **Idle State**: when no search has been submitted yet, the page displays only the search input field and the
  recent-search list. Filter chips, result shelves, and discovery content remain hidden until a search is executed.
- **Recent Searches**: displays up to 10 recent searches (newest first, case-insensitively deduplicated), persisted
  across restarts.
  - Clicking any recent search entry populates the search query and executes the search immediately.
  - Each entry features a delete button (`X`) on the right that removes only that entry from history.
  - Below the list, a `Clear history` button removes all entries at once. When history is empty, a placeholder
    indicates no recent searches.
- **Search Execution**: type your query into the input field and press `Enter` or click `Search` to view filtered
  search results.

### Detail Page

The detail view displays full tracklistings for an album, artist, or playlist:

- **Header**: displays container title and subtitle (such as artist, release year, or item count).
- **Track List**: lists all tracks in the collection with title, artist, and duration.
- **Playback**: click `Play` next to any track to push it to the playback queue and start playback immediately.
- **Navigation**: click `Back` to close the detail view and return to the previous page.
- Home shelf items in the `Search` page provide an `Open` button to navigate directly to their detail view.

### Queue Panel

The collapsible right-side queue panel manages the ordered list of upcoming tracks:

- Opened and closed from the queue icon in the player bar, docking on the right at 340px.
- Lists the queued tracks in order with the active track highlighted.
- Displays the queued track count badge on the queue icon in the player bar.
- Shows an empty-state text (`The queue is empty.`) when nothing is queued.
- Clicking a track row plays that track immediately and leaves the queue panel open.

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

The bottom player bar remains visible across all pages and provides continuous transport controls.

### Bar Modes and Customization

Users can customize the player bar appearance and element visibility under **Settings -> Player bar**:

- **Presentation Modes**:
  - `Classic`: Docked, full-width bar flush with the bottom edge of the window, separated by a thin 1px top border.
  - `Floating`: Centered, rounded pill card floating above the bottom edge with a subtle drop shadow and a 960px width
    cap.
- **Customizable Toggles**:
  - `Show artwork`: Toggles track album art thumbnail on the left. When hidden, the title and artist move flush left.
  - `Show like button`: Toggles the favorite heart icon button next to track metadata.
  - `Show shuffle button`: Toggles the shuffle order button in transport controls.
  - `Show repeat button`: Toggles the repeat cycle button (Off, All, One) in transport controls.
  - `Show skip 10s buttons`: Toggles the `-10s` and `+10s` jump chips next to the play/pause button (disabled by
    default for a minimal bar layout).
  - `Show queue button`: Toggles the queue button on the right displaying active queue count.
  - `Show volume slider`: Toggles the speaker icon and volume slider on the right. The speaker icon switches to a muted
    icon at volume 0 and serves as a mute toggle.

### Transport and Playback Controls

- **Track Info**: displays `title - artist` of the track under the cursor, or `Queue empty` when empty.
- **Playback Controls**:
  - `Previous`: skips to previous track in the queue and starts playback.
  - `Play` / `Pause`: toggles between playing and paused audio states.
  - `Next`: skips to next track in the queue and starts playback.
- **Seek Controls**:
  - `-10s`: rewinds current playback position by 10 seconds (clamped at 0).
  - `+10s`: advances playback position forward by 10 seconds.
- **Volume Slider**: interactive slider setting audio volume from 0 to 100. The speaker icon switches to a muted icon
  at volume 0, and clicking the icon mutes playback or restores the previous level (falling back to 50 when zero).
- **Position Display**: shows elapsed seconds against total duration (`<elapsed>s / <duration>s` or `<elapsed>s`).

## Keyboard Shortcuts

Global keyboard shortcuts provide transport and playback control when single-line text inputs are not focused:

| Key | Action | Context / Notes |
| --- | --- | --- |
| `Space` | Play / Pause | Toggles playback state of current track. |
| `Left Arrow` | Seek backward | Rewinds position by 10 seconds (clamped at 0). |
| `Right Arrow` | Seek forward | Advances position by 10 seconds. |
| `Up Arrow` | Volume up | Increases playback volume by 5% (up to 100%). |
| `Down Arrow` | Volume down | Decreases playback volume by 5% (down to 0%). |
| `N` | Next track | Skips to next track in queue (requires queued tracks). |
| `P` | Previous track | Skips to previous track in queue (requires queued tracks). |
| `L` | Toggle like | Adds or removes current track from liked songs. |

Keyboard shortcuts can be toggled in the `Library` preferences section and are enabled by default.

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
