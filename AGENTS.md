# AGENTS.md — moggyt

Native YouTube Music client in Rust (Slint). Hexagonal skeleton:
UI shell (`src/slint_shell/`) → pure domain (`src/domain/`) ← ports ← adapters (`src/infra/`).

## Verify (same gates as CI)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
```

Run all five before finishing. `cargo run` launches the window.

## Architecture invariants (see `docs/02-architecture.md`)

- Dependency direction: `slint_shell`/`infra` → `domain`. `domain` never touches I/O
  (`net`, `fs`, `process`, clocks, randomness); inject those as parameters or ports.
- `src/main.rs` is the only composition root. One concept, one owner module (SSOT).
- External crates hide behind ports in `src/infra/`; the binary crate exposes
  the library via `src/lib.rs` so `pub` APIs stay reachable (no `allow(dead_code)`).

## Code rules (see `docs/03-development.md`)

- TDD for `domain/` and `slint_shell/`: red test on public behavior first, then minimal
  code, refactor on green. Fakes in memory; no network/audio in tests; no `sleep`.
- CC <= 5 per function (hard limit 10); guard clauses first; DRY from 3rd repetition.
- English identifiers (verbs for functions, nouns for types); boolean predicates
  (`is_`, `has_`, `can_`); fail-loud errors (`anyhow`), no silent swallowing.
- English only: every tracked text in this repo is written in English — docs, comments,
  configs, scripts, templates, commit messages, and PR/issue text. Never introduce
  non-English text; translate instead.
- No emojis in code, scripts, configs, CI, or commits — docs/releases only.
- `Cargo.lock` is committed; never add dependencies without justification.
- Slint unknowns go to the source first: when Slint language, layout, binding,
  or compiler behavior is unclear, consult the official Slint documentation
  (<https://docs.slint.dev/>) before guessing (covers enums, layouts, callbacks,
  and property semantics). Do not invent Slint syntax from memory.
- UI chrome i18n: UI chrome uses English source keys translated via
  `crate::i18n::tr(Locale, key)` in Rust before reaching Slint. Slint renders
  translated properties verbatim with no `== "English"` language branches. Spanish
  literals live only in `src/i18n.rs`. Server content (shelf titles from the API,
  track/artist names) passes through untranslated.

## Workflow (PR-only, no exceptions)

- Direct commits and direct pushes to `main` are prohibited — for agents,
  contributors, and the repository owner alike. Every change lands via Pull Request.
- Branch from `main`: `<type>/<short-desc>` (`feat/`, `fix/`, `refactor/`, `docs/`,
  `test/`, `chore/`). One logical change per commit, PRs < 400 lines.
- Stacked PRs (a chain where each PR targets the previous branch instead of `main`):
  CI and secret scanning run on every PR, so stacked PRs show checks immediately.
  Land bottom-up: merge the lowest PR into `main` first, retarget the next PR to
  `main`, wait for its green CI, and repeat. Never merge a PR whose checks never ran.
- Fresh base before any work: `git fetch origin` first; never branch, implement,
  or open a PR from a stale base. If local `main` trails `origin/main`, sync and
  re-assess scope before starting.
- Overlap check before delegating: scan `git log origin/main` and open PRs for
  landed or in-flight work covering the same scope; never duplicate it.
- If `origin/main` advances mid-task, stop, re-sync onto the new base, re-run the
  gate, and re-confirm scope before continuing.
- Commits in English, Conventional Commits, imperative mood, no emojis.
- Update `docs/` or `README.md` in the same PR when behavior, flags, config,
  or public APIs change.
- Merge only with green CI (`fmt`, `clippy`, `test`, standards audit, markdownlint).
  Merge method is Squash (default) or Rebase — never Merge commits
  (`gh pr merge --squash`, never `--merge`; repo setting `allow_merge_commit: false`).
  Agents open PRs — the owner reviews (self-review checklist in `CONTRIBUTING.md`)
  and merges. Exception: when the owner has explicitly granted merge autonomy for
  a work sequence, the agent may merge only PRs it authored, only with all CI
  checks green, and only via Squash (`gh pr merge --squash`, never `--merge`).
  It must never merge PRs from other authors, bypass checks, or push version tags.
- Never publish releases unless explicitly asked.
- Releases are tag-triggered only: merging to main never ships; the owner cuts a release by pushing tag vX.Y.Z.
- Agents may push branches and open PRs, but must never create or push version tags unless explicitly asked.
- Never commit secrets, `.env`, debug output, or foreign files; keep the diff minimal.
