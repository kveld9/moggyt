# AGENTS.md — moggyt

Native YouTube Music client in Rust (`eframe`/`egui`). Hexagonal skeleton:
UI shell (`src/app.rs`) → pure domain (`src/domain/`) ← ports ← adapters (`src/infra/`).

## Verify (same gates as CI)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
```

Run all four before finishing. `cargo run` launches the window.

## Architecture invariants (see `docs/02-architecture.md`)

- Dependency direction: `app`/`infra` → `domain`. `domain` never touches I/O
  (`net`, `fs`, `process`, clocks, randomness); inject those as parameters or ports.
- `src/main.rs` is the only composition root. One concept, one owner module (SSOT).
- External crates hide behind ports in `src/infra/`; the binary crate exposes
  the library via `src/lib.rs` so `pub` APIs stay reachable (no `allow(dead_code)`).

## Code rules (see `docs/03-development.md`)

- TDD for `domain/` and `app/`: red test on public behavior first, then minimal
  code, refactor on green. Fakes in memory; no network/audio in tests; no `sleep`.
- CC <= 5 per function (hard limit 10); guard clauses first; DRY from 3rd repetition.
- English identifiers (verbs for functions, nouns for types); boolean predicates
  (`is_`, `has_`, `can_`); fail-loud errors (`anyhow`), no silent swallowing.
- English only: every tracked text in this repo is written in English — docs, comments,
  configs, scripts, templates, commit messages, and PR/issue text. Never introduce
  non-English text; translate instead.
- No emojis in code, scripts, configs, CI, or commits — docs/releases only.
- `Cargo.lock` is committed; never add dependencies without justification.

## Workflow (PR-only, no exceptions)

- Direct commits and direct pushes to `main` are prohibited — for agents,
  contributors, and the repository owner alike. Every change lands via Pull Request.
- Branch from `main`: `<type>/<short-desc>` (`feat/`, `fix/`, `refactor/`, `docs/`,
  `test/`, `chore/`). One logical change per commit, PRs < 400 lines.
- Commits in English, Conventional Commits, imperative mood, no emojis.
- Update `docs/` or `README.md` in the same PR when behavior, flags, config,
  or public APIs change.
- Merge only with green CI (`fmt`, `clippy`, `test`, standards audit, markdownlint).
  Merge method is Squash (default) or Rebase — never Merge commits
  (`gh pr merge --squash`, never `--merge`; repo setting `allow_merge_commit: false`).
  Agents open PRs and never merge them — the owner reviews (self-review checklist
  in `CONTRIBUTING.md`) and merges.
- Never publish releases unless explicitly asked.
- Never commit secrets, `.env`, debug output, or foreign files; keep the diff minimal.
