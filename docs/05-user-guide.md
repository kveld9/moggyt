# User Guide

This guide covers the interface layout, playback controls, queue management, and library features in `moggyt`.

## Interface Layout

The interface consists of three primary regions:

1. **Sidebar**: fixed navigation rail on the left with the application wordmark and one item per page
   (`Search`, `Explore`, `Library`, `Stats`, `Now playing`). The active item is highlighted.
2. **Content Area**: header with the active page title, a settings button on the right that opens the
   Settings page, and an update badge when a newer release is detected. Detail pages show only the back
   control in the header, because the container title lives in the detail view header next to the
   artwork. Below the header, the active page renders its content; contextual pages (`Detail`, `Import`)
   replace the current view.
3. **Player Bar**: persistent bottom panel providing transport controls, volume slider, and track position.

## Navigation Pages

Sidebar destinations are `Search`, `Explore`, `Library`, `Stats`, and `Now playing`. `Detail` and
`Settings` are contextual: `Detail` opens from a track, album, artist, or playlist selection, and
`Settings` opens from the settings button in the content header or from the Library sign-in prompt.

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
- **Top result**: when one catalog hit clearly matches the query, a `Top result` card appears above the
  result list with its artwork, name, and type. Clicking it opens that album, artist, or playlist, or plays
  the track when the top result is a song. The promoted hit is not repeated in the list below. Queries with
  no clear match show no card.
- `All`: displays results across all categories.
- `Songs`: filters results to individual tracks.
- `Albums`: filters results to albums.
- `Artists`: filters results to artists.
- `Playlists`: filters results to playlists.
- `Lyrics`: treats the query as a remembered lyric fragment. The fragment is resolved
  through lrc.red and matched against the catalog by title, artist, and duration, so
  results are real tracks rather than catalog text matches.
- **Clearing**: clearing the input (the `X` button or backspacing to empty) returns the page to the
  Idle state with the recent-search list.
- **Zero hits**: a query with no matches shows an empty-results message. A failed catalog request
  shows an error message instead of an empty list.
- **Shelf cards**: album and single cards show the release type followed by the credited artist
  under the title (`Single • Toosii`); artist and playlist cards show their own label.

### Detail Page

The detail view displays full tracklistings for an album, artist, or playlist:

- **Header**: displays container title and subtitle (such as artist, release year, or item count).
- **Track List**: lists all tracks in the collection with title, artist, and duration.
- **Artist sections**: artist pages show the popular-songs heading and its top tracks, a `Show all` action
  that opens the complete songs list, and the server-provided `Albums`, `Singles and EPs`, and similar-artist
  rows. Album and single cards open the album page; similar-artist cards open that artist's page.
- **Playback**: `Play` appends the whole collection to the playback queue and starts at its first track.
  Clicking a track row appends the collection and starts at that row, so `Next` continues through the
  listed tracks.
- **Navigation**: click `Back` to close the detail view and return to the previous page.
- Home shelf items in the `Search` page provide an `Open` button to navigate directly to their detail view.

### Queue Panel

The collapsible right-side queue panel manages the ordered list of upcoming tracks:

- Opened and closed from the queue icon in the player bar, docking on the right at 340px.
- Lists the queued tracks in order with the active track highlighted.
- Displays the queued track count badge on the queue icon in the player bar.
- Shows an empty-state text (`The queue is empty.`) when nothing is queued.
- Clicking a track row plays that track immediately and leaves the queue panel open.
- **Clear Queue**: click the `Clear queue` button in the header row to empty the queue and stop
  playback; the empty queue is persisted across restarts.

### Now Playing Page

The Now Playing view presents a full two-column playback dashboard:

- **Left Column**: a fixed 340px column featuring a prominent 220x220 artwork block, track details,
  transport controls, and volume adjustment.
  - **Cover & Vinyl Deck**: toggled with `Cover` and `Vinyl` buttons. Cover mode displays album
    artwork (or a placeholder when unavailable); Vinyl mode renders an animated turntable stage whose record
    spins and tonearm swings into place during active playback.
  - **Fix Audio Version**: the `Fix version` button opens a search panel for alternative recordings
    of the current track. The chosen alternative is stored in `version_overrides.json` and used for
    playback only; the title, artwork, lyrics and scrobbling keep the original track.
  - **Track Details**: large track title and artist name with text elision.
  - **Favorites**: circular like button toggling favorite status through `PlayerUi`. The favorite
    set is persisted in `settings.json` across restarts.
  - **Transport Controls**: playback center controls featuring play/pause, previous/next, shuffle,
    repeat, track position timeline, and +/-10s seek buttons.
  - **Volume Row**: speaker icon with one-click mute/unmute and volume slider (`0-100`).
- **Right Card**: a rounded surface card equipped with `Lyrics` and `Up next` tabs.
  - **Lyrics Tab**: displays synchronized lyrics with active-line accenting and word-level sung/rest
    highlighting when available, or an empty state hint when lyrics are missing.
  - **Up Next Tab**: lists all queued tracks with active track highlighting; clicking a row jumps
    directly to that queue position. Displays an empty state message when the queue has no tracks.

### Library Page

The library view lists the saved collection and hosts the import entry point. Session controls live in
the Settings page, not here:

- **Not signed in**: shows `Sign in from Settings to load your library.` next to an `Open settings`
  button that switches to the Settings page, and hides the playlist and album sections.
- **Authentication**: handled in `Settings` > `Session`, where the cookie string is pasted, stored in the
  OS credential store (keyring), and cleared on sign-out.
- **Updates Section**:
  - Displays current update status (`Never checked`, `moggyt is up to date`, `Update available: <version>`, or
    `Update check failed (offline?)`).
  - `Check for updates`: triggers an asynchronous query to GitHub Releases.
  - When an update is available, displays a direct link to the GitHub download page and a `Dismiss` button to clear the
    notice for the session.
- **Playlists**: lists saved playlists with title and track counts. Shows `No playlists` when empty. When signed in,
  a read-only "History" entry appears first in the list, loading the account's recent plays as recorded by YouTube
  Music into the detail view with grouped day sections flattened in order.
- **Liked Songs**: lists liked tracks. Each row provides track details and an `Unlike` button.
- **Saved Albums**: lists saved albums by title and artist. Shows `No saved albums` when empty.

### Stats Page

The stats page presents Wrapped-style listening statistics computed from your local listening history:

- **Navigation**: accessible directly from the sidebar by selecting the `Stats` item.
- **Selectable Periods**: choose between `Last 4 weeks`, `Last 6 months`, and `All time` using the top filter chips.
- **Qualification Criteria**: tracks you listen to for at least 30 seconds are counted as valid plays.
- **Summary Metrics**: view total plays, cumulative listening time, unique track count, and unique artist count.
- **Top Rankings**: inspect top 5 artists and top 5 tracks ranked by play count and listening duration.
- **Local Data Privacy**: all metrics are computed locally from `play-log.jsonl` without contacting external servers.

### Spotify Import

`moggyt` allows importing playlists and albums from Spotify directly into your local library, resolving tracks against
YouTube Music audio streams without requiring Spotify accounts or developer API keys.

#### Launching Import

Click the `Import` button located in the `Library` view to open the Spotify import modal.

#### Import Sources

The modal provides two tabs for supplying playlist metadata:

- **From link (`From link`)**: Paste a public Spotify playlist or album web link or Spotify URI:
  - Standard URLs: `https://open.spotify.com/playlist/<id>` and `https://open.spotify.com/album/<id>`
  - Localized URLs: `https://open.spotify.com/intl-en/playlist/<id>`
  - Embed URLs: `https://open.spotify.com/embed/playlist/<id>`
  - Legacy user URLs: `https://open.spotify.com/user/<user>/playlist/<id>`
  - Desktop URIs: `spotify:playlist:<id>` and `spotify:album:<id>`
  Spotify identifiers must be valid 22-character alphanumeric IDs. Live validation feedback displays below the
  field ("Valid Spotify link" or "Invalid Spotify link") to verify format correctness before starting the import.
- **From file (`From file`)**: Specify the path to an exported playlist file. Clicking the `...` browse button opens a
  native file dialog filtered to `.csv` and `.json` files, or you can type the file path directly into the input field:
  - **Exportify CSV**: 19-column CSV file exported from Exportify containing `Track Name`, `Artist Name(s)`,
    `Album Name`, `Track Duration (ms)`, and `Track URI` columns (`ISRC` is optional and, when present, is used as the
    first search query).
  - **Spotify JSON Backup**: Standard JSON playlist backup file containing track and artist metadata.

#### Import Options

The import modal provides additional configuration before initiating track resolution:

- **Playlist Name (Optional)**: Specify a custom title for the imported playlist. When provided, this name
  overrides the source title when saving to your local library. If left empty, the original Spotify playlist or album
  title is preserved. The name override is hidden and ignored when loading directly to the queue.
- **Load directly to queue**: When enabled via the toggle switch, resolved tracks are appended directly to the current
  playback queue and playback begins at the first imported track, without saving a local playlist to your library.

#### Embed 100-Track Limit & CSV Alternative

Spotify embed pages limit returned tracklists to the first 100 tracks. For public playlists with over 100 tracks, or
for private playlists, export the playlist using Exportify or a Spotify backup tool, and import the resulting CSV or
JSON file using the **From file** tab to import all tracks without truncation.

#### Spotify Web API Credentials (Optional)

By default, link imports scrape Spotify embed pages without requiring any credentials. To import playlists longer than
100 tracks directly from links or leverage ISRC catalog identifiers for improved matching:

1. Create a developer application in the Spotify Developer Dashboard (<https://developer.spotify.com/dashboard>).
2. Copy your **Client ID** and **Client Secret**.
3. In moggyt, open **Settings -> Integrations -> Spotify Web API**.
4. Paste the credentials into the respective input fields and click **Save credentials**.

When saved, credentials are held securely in your OS credential manager (`Secret Service` on Linux, `Credential
Manager` on Windows, or `Keychain` on macOS). With complete credentials configured, link imports automatically use the
official Spotify Web API to fetch full playlists without the 100-track cap and match tracks using ISRC identifiers
first. If credentials are absent or cleared via **Clear credentials**, link imports seamlessly fall back to the public
embed page scraper.

#### Progress Overlay

Clicking `Start import` initiates the matching process and displays the progress overlay:

- **Progress Bar**: Shows real-time progress (`done / total` tracks).
- **Status Tallies**:
  - **Matched**: Number of high-confidence matches found.
  - **Needs review**: Number of medium-confidence matches requiring visual verification.
  - **Unresolved**: Number of tracks where no candidate scored above the minimum threshold (0.50) or the track is
    absent from YouTube Music.
- **Cancel Button**: Clicking `Cancel` stops matching at any time; processed tracks are retained and forwarded to the
  review screen.

#### Review Step & Confidence Badges

After matching completes or is cancelled, the review screen presents each Spotify track alongside its matched YouTube
Music candidate:

- **Connection Lost Banner & Retry**: If the network connection drops during matching, the pipeline stops immediately
  and presents the review screen with the tracks matched so far. A "Connection lost. Retry or save the matched tracks."
  banner appears with a `Retry` button. Clicking `Retry` re-runs matching from the beginning, while clicking `Save
  playlist` keeps the tracks matched so far.
- **Exclude Checkbox**: A checkbox on the left of each row allows excluding specific tracks from import. Excluded
  rows are dimmed and skipped entirely when saving to your library or loading into the queue.
- **Matched Channel & Duration Delta**: The matched candidate shows its YouTube channel or artist name beneath the
  title, accompanied by a signed duration delta (e.g. `+3s`, `-2s`, `0s`) comparing source and matched durations.
- **High Confidence (`Matched` badge)**: Score `>= 0.85`. High-confidence match where title, artist, and duration align
  closely with official audio tracks.
- **Medium Confidence (`Needs review` badge)**: Score in `[0.60, 0.85)`. Acceptable match that may differ slightly in
  version, release edition, or artist attribution.
- **Unresolved**: No candidate reached the 0.50 threshold. Displays the original Spotify title and artist; omitted from
  the imported playlist unless manually swapped.

#### Swapping Matches

If a matched track is incorrect or a better alternative exists:

1. Click the `Swap` button on the track row to open the alternative picker.
2. The picker presents up to five ranked alternative candidates found during search, ordered by match score.
3. Select an alternative to replace the candidate for that row.
4. Alternatively, paste an explicit 11-character YouTube video ID into the custom ID field and click `Use ID` to
   force that track for the row. This applies to both matched and unresolved tracks.

#### Auditioning Matches

Click the `Open` button on any matched review row or alternative candidate to open its track on
YouTube Music in the default web browser. Auditioning a track does not alter the selected match
or row status. If the browser fails to open, an error message is reported in the status line.

#### Saving and Persistence

- Click `Save playlist` to persist the imported playlist, or `Discard` to cancel without saving.
- Saved playlists are written atomically to `playlists.json` in the configuration directory (`~/.config/moggyt/` on
  Linux, `%APPDATA%\moggyt` on Windows).
- Saved playlists are assigned identifiers prefixed with `local:` (e.g. `local:<id>`) and appear in the `Library` page
  under the `Playlists` section.
- Clicking an imported playlist opens the `Detail` view, allowing playback without contacting YouTube Music for catalog
  resolution. Opening an imported playlist displays `Rename` and `Delete` controls in the header, and each track row
  features compact `Up`, `Down`, and `Remove` buttons. Any edits, deletions, track removals, or reorder changes
  persist immediately to `playlists.json`.

#### Headless CLI Usage

You can also import playlists headlessly from the terminal without opening the graphical interface. The CLI
runs before and without taking the single-instance lock or initializing audio/tray devices, so imports work even
while the GUI is actively running:

```bash
# Import from a Spotify link (--headless is optional and implied)
moggyt --import-spotify <URL>

# Import and override the saved playlist title
moggyt --import-spotify <URL> --name "Custom Title"

# Import from an Exportify CSV or Spotify JSON backup file
moggyt --import-csv <PATH>

# Emit machine-readable JSON output to stdout (--json requires --headless)
moggyt --import-spotify <URL> --headless --json
moggyt --import-csv <PATH> --headless --json

# Import tracks from a Spotify URL directly into the GUI playback queue
moggyt "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"
```

In headless mode:

- The `--headless` flag is optional and implied by `--import-spotify` and `--import-csv`.
- Note the parser rule: `--json` still requires `--headless` (`--json requires --headless`), so keep `--headless`
  in every `--json` invocation.
- Use `--name "Custom Title"` to override the playlist title before saving it to the library.
- Passing a positional Spotify URL (e.g. `moggyt "https://open.spotify.com/playlist/..."`) resolves tracks
  synchronously, prints progress to `stderr`, and enqueues matched tracks directly into the GUI playback queue
  without saving a playlist to disk.
- Operates before single-instance lock acquisition, allowing CLI imports while the GUI is running (a running GUI
  does not refresh its library live; saved playlists become available on restart).
- `--import-csv` accepts both Exportify CSV files and Spotify JSON backup files.
- Matching progress is streamed to `stderr`.
- A summary report is printed to `stdout` upon completion (JSON format with `--json`, plain text otherwise).
- Successfully imported playlists are saved directly to `playlists.json` in the configuration directory.

### Playback Settings

Playback options can be configured under **Settings -> Playback**:

- **Prefer songs over music videos**: When enabled, filters catalog search results and autoplay recommendations
  to prefer audio tracks over music videos.
- **Autoplay similar tracks**: Appends similar tracks when the queue ends. It is off by default; when enabled,
  playback continues with catalog radio recommendations after the last track.

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
  - **Device Change**: playback pauses automatically when the system default output
    device changes (for example, unplugging headphones); the pause fires within about
    one second. On Linux the PulseAudio/PipeWire default sink is tracked through the
    audio server event stream when `pactl` is available, with a five-second safety
    poll; otherwise the audio device name is polled every second. Press `Play` to
    resume on the new device.
  - `Next`: skips to next track in the queue and starts playback.
  - `Shuffle`: randomizes the queue while keeping tracks from the same artist apart
    and moving tracks from your most recent listening sessions toward the end of the
    queue. With only one artist in the queue it falls back to a plain shuffle.
- **Seek Controls**:
  - `-10s`: rewinds current playback position by 10 seconds (clamped at 0).
  - `+10s`: advances playback position forward by 10 seconds.
- **Volume Slider**: interactive slider setting audio volume from 0 to 100. The speaker icon switches to a muted icon
  at volume 0, and clicking the icon mutes playback or restores the previous level (falling back to 50 when zero).
- **Position Display**: shows the elapsed time on the left of the timeline and the track duration on the right, both
  formatted as `m:ss`. Clicking the duration label toggles it between the total duration and the time remaining
  (rendered with a leading minus, e.g. `-2:13`). The label shows `--:--` until the track duration is known from
  catalog metadata or the decoded audio stream.
  The elapsed label tracks audible playback, so it stays frozen while playback is
  paused and never shows a position longer than the displayed duration.

## Context Menu

Track rows across the app expose a right-click context menu rendered as a themed popup anchored at
the pointer:

- **Track lists**: `Search` results, detail track listings, the queue panel, and the Now Playing
  `Up next` list.
- **Play**: queues the track and starts playback on it immediately.
- **Play next**: inserts the track immediately after the current queue position, leaving the
  current playback untouched.
- **Add to queue**: appends the track to the end of the queue.
- **Go to artist** and **Go to album**: open the matching detail page for the row. The container is
  resolved by name against the catalog off the UI thread; when nothing matches, a toast reports the
  failure. These entries appear when the row carries an artist.
- **Like**: adds the track to your local favorites and switches the entry to `Unlike` while the
  track is already liked. Favorites are stored in `settings.json` and survive restarts; they are
  device-local and are never synced with your YouTube Music account.
- **Copy link**: copies the track's YouTube Music URL (`https://music.youtube.com/watch?v=<id>`) to
  the system clipboard.
- **Cards**: album, artist, and playlist rows and cards (search results, `Explore` and category
  grids, artist-page sections, and the `Library` lists) open a menu with `Open`, which navigates to
  their detail page. Album cards whose catalog payload credits an artist also offer `Go to artist`,
  which opens that artist's page.
- The menu closes when an entry is chosen, when you press anywhere outside the popup, or when you
  press `Esc`. Left-clicking a card still opens its detail page directly.
- Switching to another application leaves the popup open. It closes on the first click after you
  return, or with `Esc`.

Context actions confirm their result with a transient toast in the bottom-right corner, above the
player bar: link copied, copy failure, added to the queue, playing next, and favorite added or
removed. Each toast shows the action's icon, fades and slides in, hugs its message, stacks up to
three, and disappears after three seconds while a thin accent line along its bottom edge shrinks to
zero.

## Keyboard Shortcuts

Keyboard shortcuts are always enabled and cannot be disabled. They provide transport and playback control whenever a
text input does not have focus. Any shortcut pressed with Ctrl, Alt, or Meta held is ignored.

| Key | Action | Context / Notes |
| --- | --- | --- |
| `Space` | Toggle play/pause | Toggles playback state of current track. |
| `Left` | Seek -10s | Rewinds playback position by 10 seconds (clamped at 0). |
| `Right` | Seek +10s | Advances playback position forward by 10 seconds. |
| `Up` | Volume +5% | Increases playback volume by 5% (max 100). |
| `Down` | Volume -5% | Decreases playback volume by 5% (min 0). |
| `N` | Next track | Skips to next track in queue (requires queued tracks). |
| `P` | Previous track | Skips to previous track in queue (requires queued tracks). |
| `L` | Toggle like | Adds or removes current track from liked songs. |
| `Esc` | Return focus | Returns focus from an active text input so shortcuts apply again. |

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

- **Discord Rich Presence**: opt-in toggle in Settings > Integrations. Broadcasts a "Listening to moggyt" activity with
  title and artist; connects lazily when Discord is running (also if Discord starts later, on the next track change).
  Disabling clears the status.
- **Last.fm Scrobbler**: initialized in a dormant state by default until credentials and registration are configured.
