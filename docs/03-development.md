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

- Rust (`.rs`): max 800 lines for every Rust file, with no exemptions.
- Slint (`.slint`): max 400 lines per component module.
- Thin entry exemption: `ui/shell.slint` is exempt only while it stays an import-only entry under ~200 lines
  with no component definitions.
- Remediation path: split oversized files by concept (one view or endpoint-domain per module), landing each
  extracted unit via one small PR per module (< 400 lines diff).

## Complexity limits

Functions target cognitive complexity CC <= 5 (hard limit 10), using guard clauses first.
The blocking CI gate runs on production targets only:

```bash
cargo clippy --locked --lib --bins --all-features -- -D clippy::cognitive_complexity
```

Test modules are exempt because `--lib --bins` excludes test harnesses; test suites are assertion tables
rather than application logic.

Clippy's `cognitive_complexity` counts control flow expanded from macros (notably `tracing`), so functions
whose count is an expansion artifact carry an explicit `#[allow(clippy::cognitive_complexity)]` with a NOTE
recording the real branch count. Upstream references:

- rust-clippy#14417: <https://github.com/rust-lang/rust-clippy/issues/14417>
- rust-clippy#7905: <https://github.com/rust-lang/rust-clippy/issues/7905>
- tokio-rs/tracing#553: <https://github.com/tokio-rs/tracing/issues/553>

Rule for new code: refactor first, annotate only when the count is provably an expansion artifact.

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

## Slint lessons

Distilled rules from Slint behavior that surprised us. Consult the linked source before
guessing; every rule here was verified against a build or the official documentation.

- A card grid built from a runtime model cannot use `GridLayout` `row` / `col`: both are
  cell properties whose bindings must be compile-time constants, so an index-derived grid
  (`row: i / 5`) does not compile. Use one `Flickable` whose `viewport-width` is computed
  from the model length, with a single `HorizontalLayout` of fixed-size cards inside.
  Source: <https://docs.slint.dev/latest/docs/slint/reference/layouts/gridlayout/>.
- Rows of cards only wrap when each card has a real width: a `HorizontalLayout` of cards
  without a width sizes them from their own preferred width, so the row overflows the window
  and gets clipped, and `preferred-width: 0px` collapses it to zero instead. Give the card a
  fixed `width` and chunk the model into rows of the cards that fit the window width, then
  recompute that chunking from a `changed width => { ... }` handler on the `Window`, which
  Slint fires on every resize and is the only way to reflow without a scroll offset.
  Source: <https://docs.slint.dev/latest/docs/slint/reference/layouts/horizontallayout/>.
- `FlexboxLayout` (`flex-wrap` defaults to `wrap`) is the documented element for a grid that
  must wrap, but it is not a drop-in inside this app's explore `Flickable`: on slint 1.18.1
  the four wrapping containers reported their exact heights (844/630/1486/844px) and the
  `ExploreView` reported 3980px, while the enclosing scroll layout kept 4735px for children
  summing 4013px, so the page ended with about 640px (three card rows) of dead scroll space
  after a window resize. Confirmed with `debug()` on `changed height` handlers, no
  `GridLayout` involved. Keep the fixed-width cards plus width-derived chunking until this
  is fixed upstream, and re-run the bottom-of-scroll check before switching to
  `FlexboxLayout`.
  Source: <https://docs.slint.dev/latest/docs/slint/reference/layouts/flexboxlayout/>.
- Setting layout `alignment` cancels child `horizontal-stretch` / `vertical-stretch`: by default,
  `HorizontalLayout` and `VerticalLayout` use `alignment: stretch`, sizing children to their minimum size and
  distributing remaining space according to each child's stretch factor. Specifying an explicit alignment (such as
  `space-between` or `center`) sizes children only to their preferred/minimum size and places extra space into gaps,
  preventing children with `horizontal-stretch: 1` from expanding. To let a center child stretch between fixed-width
  edge elements, omit `alignment` on the container layout.
  Source: <https://docs.slint.dev/latest/docs/slint/src/language/builtins/layouts>.
- `Flickable` viewport content-sizing and layout interaction: a `Flickable` does not provide an intrinsic
  content height to enclosing layouts or intermediate containers; its `viewport-height` is computed from its
  children, while its own outer `height` defaults to 100% of the parent element or must be allocated by layout stretch.
  Wrapping a `Flickable` inside an intermediate `Rectangle` with circular height constraints (`height: parent.height`)
  or placing it in a layout with explicit alignment (such as `alignment: start`) collapses the container to zero height
  because `vertical-stretch` is ignored and `Flickable`'s preferred height is zero. To allow vertical scrolling without
  clipping or collapse, place `Flickable` directly in the stretching layout with `vertical-stretch: 1` (leaving the parent
  layout at default `stretch` alignment). Inside `Flickable`, keep the inner `VerticalLayout` free of explicit `height`
  and explicit `width` so its height follows content rows naturally, enabling `viewport-height` to be derived accurately.
  Empty states that do not scroll should be rendered outside `Flickable` in a dedicated layout with `vertical-stretch: 1`
  so they cannot collapse when the list is empty.
  Source: <https://docs.slint.dev/latest/docs/slint/src/language/builtins/elements#flickable>.
