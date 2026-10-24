# 03 - Development and Verification

## Toolchain

- Stable Rust (`rustup default stable`), `rustfmt` and `clippy` components.
- `Cargo.lock` is committed: it is the single source of truth for reproducible versions.

## Commands

```bash
cargo run                                  # run the app (eframe window)
cargo test --all-features                  # full suite
cargo fmt --all -- --check                 # formatting (CI fails on drift)
cargo clippy --all-targets --all-features -- -D warnings
./scripts/check-repo.sh                    # repo standards audit
./scripts/check-markdown.sh                # markdown lint (CI fails on drift)
```

CI additionally runs markdown quality checks using `markdownlint-cli2`.

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

## Local pre-commit (recommended)

```bash
cp .githooks/pre-commit.example .githooks/pre-commit
chmod +x .githooks/pre-commit
git config core.hooksPath .githooks
```
