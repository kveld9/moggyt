# Troubleshooting

This guide provides diagnosis and remediation steps for common operational issues encountered in `moggyt`.

## Diagnostic Matrix

| Symptom | Underlying Cause | Solution |
| --- | --- | --- |
| Audio fails or runs silently without output | `rodio` output stream initialization failed (missing ALSA/PulseAudio/WASAPI device); client fell back to muted stub | Verify host audio hardware. Launch with `RUST_LOG=debug` to view the audio initialization error log. |
| Stream resolution fails or logs extraction warning | Upstream `yt-dlp` warns that YouTube extraction without a JavaScript runtime is deprecated | Install a supported JavaScript runtime (`deno`) in the host system PATH. |
| Cookie save fails or crashes on Linux | No compatible Secret Service keyring provider installed or running on D-Bus | Ensure a Secret Service provider is active (e.g. `gnome-keyring`, `ksecretsservice`, or `keepassxc`). |
| System tray icon does not appear | Desktop environment or compositor does not support Ayatana / AppIndicator protocols | Normal behavior; the app logs a warning and runs fully functional without a tray icon. |
| Cover art shows "Cover art unavailable" | Network request failed, track lacks artwork, or offline | Verify internet connection; artwork will reload automatically when available. |
| Update check shows "Update check failed (offline?)" | GitHub API unauthenticated request returned 404 (expected for private repo pre-launch) | Expected behavior prior to public release launch per ADR-001. No action required. |
| Inbound multimedia keys or media buttons do not control playback | MPRIS / SMTC inbound key routing is currently attach-no-op | Known limitation in v1: MPRIS and SMTC publish outbound metadata only; inbound controls are deferred. |
| "Queue is empty" appears in Now Playing view | Playback attempted without queued tracks | Navigate to the Search page, type a query, and press Enter to enqueue a track first. |

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
