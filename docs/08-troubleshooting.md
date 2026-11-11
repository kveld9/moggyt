# Troubleshooting

This guide provides diagnosis and remediation steps for common operational issues encountered in `moggyt`.

## Diagnostic Matrix

| Symptom | Underlying Cause | Solution |
| --- | --- | --- |
| Audio fails or runs silently without output | `rodio` output stream initialization failed (missing ALSA/PulseAudio/WASAPI device); client fell back to muted stub | Verify host audio hardware. Launch with `RUST_LOG=debug` to view the audio initialization error log. |
| Stream resolution fails or logs extraction warning | Upstream `yt-dlp` warns that YouTube extraction without a JavaScript runtime is deprecated | Install a supported JavaScript runtime (`deno`) in the host system PATH. |
| Playback starts after a several-second delay and logs a bot-check fallback | InnerTube player request challenged with "Sign in to confirm you're not a bot"; all unauthenticated clients receive it for that video or network | Expected fallback: playback continues through the yt-dlp sidecar. Keep the sidecar provisioned and install `deno` for full format coverage. |
| Cookie save fails or crashes on Linux | No compatible Secret Service keyring provider installed or running on D-Bus | Ensure a Secret Service provider is active (e.g. `gnome-keyring`, `ksecretsservice`, or `keepassxc`). |
| System tray icon does not appear | Desktop environment or compositor does not support Ayatana / AppIndicator protocols | Normal behavior; the app logs a warning and runs fully functional without a tray icon. |
| Cover art shows "Cover art unavailable" | Network request failed, track lacks artwork, or offline | Verify internet connection; artwork will reload automatically when available. |
| Update check shows "Update check failed (offline?)" | GitHub API unauthenticated request returned 404 (expected for private repo pre-launch) | Expected behavior prior to public release launch per ADR-001. No action required. |
| Inbound multimedia keys or media buttons do not control playback | Another running application or browser tab has acquired exclusive focus on media keys or desktop presence daemon | Close conflicting players or browser tabs, or verify desktop environment media key shortcut bindings. |
| "Queue is empty" appears in Now Playing view | Playback attempted without queued tracks | Navigate to the Search page, type a query, and press Enter to enqueue a track first. |

## Playback Falls Back After a Bot Check

YouTube intermittently challenges the unauthenticated InnerTube player request for a
video or a network with `LOGIN_REQUIRED` and the reason "Sign in to confirm you're not
a bot". The `VISIONOS`, `ANDROID_VR`, and `MWEB` clients all receive the same challenge,
so there is no client switch that bypasses it without authenticated access.

moggyt classifies the response as `bot_check`, logs
`innertube stream resolution failed; using yt-dlp fallback` once per cause, and resolves
the track through the yt-dlp sidecar. Playback still starts; the extra delay is the
sidecar extraction (measured at about eight seconds on a challenged video) rather than a
playback failure.

No action is required while playback starts. If it does not, verify that the sidecar
binary exists and install `deno` so the extractor can run its JavaScript stage.

## Inspecting Runtime Logs

To diagnose runtime errors in detail, run the executable from a terminal with `RUST_LOG` set to `debug` or `trace`:

```bash
RUST_LOG=moggyt=debug cargo run
```

Key log events to examine:

- `audio initialization failed; running with muted stub`: audio output device could not be acquired.
- `primary cover cache failed at ...; using temp fallback`: primary data directory permissions issue.
- `desktop media presence live: false`: MPRIS or SMTC service could not attach.
- `tray unavailable; running without system tray icon`: platform lacks system tray daemon.

## Keyring Issues on Linux Headless / Minimal Environments

When running on minimal window managers (e.g. i3, sway) without a desktop environment session manager, the Secret
Service daemon may not launch automatically. Ensure `gnome-keyring-daemon --start` or equivalent runs at session startup
if persistent cookie storage is required.

## "moggyt is already running"

On Linux, the single-instance lock is an abstract Unix socket released automatically by the kernel on process
exit. Helper processes spawned by moggyt cannot keep the socket bound after the application closes because the
socket is created close-on-exec (`SOCK_CLOEXEC`). If another launch reports that moggyt is already running, a
leftover instance is still active in the background and can be terminated with `pkill -x moggyt`.
