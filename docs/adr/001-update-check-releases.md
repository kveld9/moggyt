# ADR-001: Update Check against GitHub Releases on Private Repository

## Status

Accepted (2026-10-24): Option A (public releases at v1 launch).

## Context

`moggyt` ships an update check mechanism (FR-12) via the `Updater` port and `GithubUpdater` adapter. On user request,
the client queries `https://api.github.com/repos/kveld9/moggyt/releases/latest`. The adapter sends no authentication
headers by design to keep desktop client operations anonymous and avoid bundling credentials.

The GitHub repository is currently private (verified via `gh`). Release `v1.0.0` already exists on the repository.
Because GitHub requires authentication to view private repository resources, unauthenticated API queries receive an
HTTP 404 response. The updater does not fail loudly: it soft-fails by returning `None`, which transitions the UI
state to `UpdateStatus::CheckFailed` and displays "Update check failed (offline?)" without interrupting playback.

## Decision Drivers

- Zero user friction: desktop clients should not require end users to configure GitHub Personal Access Tokens (PAT).
- Security and credential hygiene: client binaries must never embed private tokens or secrets.
- Minimal complexity: preserve the existing unauthenticated adapter architecture if possible.
- Support lifecycle: distinguish pre-launch private development from public v1 distribution.

## Options

### Option A: Public releases at launch

Make repository releases or the repository itself public upon v1 launch.

- Pros: Requires zero code or configuration changes; existing `GithubUpdater` functions immediately on launch.
- Cons: Update check remains in `CheckFailed` state throughout private pre-launch testing.

### Option B: Add token support in the app

Extend the application to accept a GitHub PAT via settings or keyring to authenticate release API requests.

- Pros: Allows update verification against private releases during pre-launch development.
- Cons: Adds UI complexity, credential management overhead, and poor UX for regular users.

### Option C: Drop the update check

Remove FR-12, the `Updater` port, `GithubUpdater` adapter, and related UI elements entirely.

- Pros: Eliminates external GitHub API dependency and pre-launch failure state.
- Cons: Loses update discoverability for desktop users; requires code removal.

## Decision

Accept Option A (public releases at v1 launch). The current unauthenticated implementation is correct for public
distribution, and soft-failing to `CheckFailed` during private pre-launch is safe and non-blocking. Option B is
deferred unless private distribution channels become a concrete requirement.

## Consequences

- No client changes needed; checks succeed once releases are public.
- Pre-launch private testing stays in `CheckFailed` until launch; this is expected and non-blocking.
- Option B (token support) stays deferred unless private distribution becomes a requirement.
- Option C (removing FR-12) is rejected.
