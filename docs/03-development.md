# 03 - Development and Verification

## Toolchain

- Stable Rust (`rustup default stable`), `rustfmt` and `clippy` components.
- `Cargo.lock` is committed: it is the single source of truth for reproducible versions.

## Commands

```bash
cargo run                                  # run the app (Slint window)
cargo test --all-features                  # full suite
cargo fmt --all -- --check                 # formatting (CI fails on drift)
cargo clippy --all-targets --all-features -- -D warnings
./scripts/check-repo.sh                    # repo standards and modularity audit
./scripts/check-markdown.sh                # markdown lint (CI fails on drift)
```

CI additionally runs markdown quality checks using `markdownlint-cli2`.

## Modularity limits

Enforced automatically by `./scripts/check-repo.sh`:

- Rust (`.rs`): max 900 lines per file (baseline cap adjusted from suggested 800 lines to fit
  `src/infra/ytm.rs` at 891 lines).
- Slint (`.slint`): max 400 lines per component module.
- Thin entry exemption: `ui/shell.slint` is exempt only while it stays an import-only entry under ~200 lines
  with no component definitions.
- Grandfathered exceptions: `src/slint_shell/app.rs`, `src/infra/catalog_client.rs`, and `ui/shell.slint`
  (while over limit) are grandfathered pending modularization.
- Remediation path: split oversized files by concept (one view or endpoint-domain per module), landing each
  extracted unit via one small PR per module (< 400 lines diff).

## Complexity limits

Functions target cognitive complexity CC <= 5 (hard limit 10), using guard clauses first.
CI runs an advisory step (`cargo clippy --locked --all-targets --all-features -- -W clippy::cognitive_complexity`)
that reports annotations without failing the build (`continue-on-error: true`) because legacy hotspots exist.
The plan is to switch this gate to deny once existing hotspots are refactored.

## Branches and commits

- Default branch `main`. Every change enters via Pull Request from `<type>/<short-desc>`
  (`feat/`, `fix/`, `refactor/`, `docs/`, `test/`, `chore/`).
- Commits in English, imperative mood, Conventional Commits format: `feat(player): add repeat-one mode`.
- One logical change per commit; PRs under 400 lines; update docs when behavior, flags, or config changes.

## TDD (mandatory for `domain/`)

1. Red test on the public contract (observable behavior, no private state).
2. Minimal implementation to green.
3. Refactor on green (complexity and naming), re-running the suite.

Tests use in-memory fakes (fake queues and catalogs), never the real network or audio.
Time and randomness are injected; `sleep` and clock flakiness are forbidden.

## Test policy

Tests are written alongside the implementation, not after; every new public behavior needs a unit test
in the same PR. Tests use in-memory fakes, never the real network or audio. Time and randomness are injected;
`sleep` and clock flakiness are forbidden. The only test-exempt files are the designated logic-free roots
(`src/domain/mod.rs`, `src/infra/mod.rs`, `src/lib.rs`, and `src/main.rs`).

## Local pre-commit (recommended)

```bash
cp .githooks/pre-commit.example .githooks/pre-commit
chmod +x .githooks/pre-commit
git config core.hooksPath .githooks
```
