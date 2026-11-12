# 01 - Vision and Scope

## Problem

Web-based YouTube Music clients consume too much memory and depend on the browser.
`moggyt` is a fast, lightweight native desktop client written in Rust.

## Scope (v1 implementation)

- Native Slint interface with responsive desktop navigation (Search, Explore, Library, Stats, Now Playing,
  Detail, Settings).
- Pure domain core: playback queue, smart shuffle, playback phase state machine, search models, listening stats,
  Spotify matching heuristics, and local playlist models without I/O.
- Hexagonal architecture: `Catalog`, `AudioBackend`, `StreamResolver`, `LyricsSource`, `CoverCache`, `SessionStore`,
  `SettingsStore`, `PlaylistStore`, `PlayLogStore`, `SkipStore`, `VersionOverrideStore`, `Scrobbler`, `Presence`,
  and `Updater` ports.
- Robust adapters: InnerTube VisionOS stream resolver with `yt-dlp` sidecar fallback, `rodio` dual-sink crossfade
  and DSP pipeline (rubato resampling, libopus decoding, loudness normalization, skip-silence), LRCLIB + lrc.red
  lyrics cascade, Last.fm scrobbling, Discord Rich Presence, MPRIS/SMTC media keys, system tray, OS keyring
  credentials, and zero-key Spotify playlist/album import.
- Local library and playlist management with in-place mutation (rename, reorder, delete) and atomic JSON persistence.

## Out of scope (for now)

- Offline audio file downloads manager and ID3 tag export.
- Remote device casting (Chromecast, DLNA, AirPlay).
- Video rendering and podcast feeds.
- Store packaging (Flatpak, MSI, Snap, App Store); releases are distributed via GitHub Releases (.AppImage, .deb,
  tarball, NSIS .exe, zip) and Arch PKGBUILD.
- Third-party plugin host / public plugin SDK.

## Guiding principles

1. Native first: fast startup, single binary, no web runtime.
2. Pure and testable domain: queue and search logic is tested without network or audio.
3. Minimal change per PR: each integration (API, audio, UI) lands in small, reviewable atomic steps.
