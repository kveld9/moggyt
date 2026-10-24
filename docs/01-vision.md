# 01 - Vision and Scope

## Problem

Web-based YouTube Music clients consume too much memory and depend on the browser.
`moggyt` is a fast, lightweight native desktop client written in Rust.

## Scope v0.1 (current scaffold)

- Native `eframe`/`egui` window with minimal navigation (search, queue, now playing).
- Pure domain core: playback queue and search models, no I/O.
- Ports (`trait`) for the YouTube Music catalog and the audio backend; real adapters land in later phases.

## Out of scope (for now)

- Offline downloads and local library management.
- Casting to remote devices.
- Store packaging (Flatpak, MSI, dmg); only `cargo build --release` binaries and GitHub releases.

## Guiding principles

1. Native first: fast startup, single binary, no web runtime.
2. Pure and testable domain: queue and search logic is tested without network or audio.
3. Minimal change per PR: each integration (API, audio, UI) lands in small, reviewable atomic steps.
