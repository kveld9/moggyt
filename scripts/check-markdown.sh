#!/usr/bin/env bash
# ==============================================================================
# Markdown Quality Gate (moggyt)
#
# Mirrors the CI "Markdown Quality Check" job locally so pushes never go out
# with lint drift. Uses the same config (.markdownlint.yaml) and the same
# linter major (markdownlint-cli2 v0.24, pinned) as
# DavidAnson/markdownlint-cli2-action@v24.
#
# Usage:
#   ./scripts/check-markdown.sh
#
# Exit codes:
#   0 - No markdown issues.
#   1 - Lint issues found, or the linter could not run.
# ==============================================================================

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

if ! command -v npx >/dev/null 2>&1; then
  echo "Error: 'npx' (Node.js) is required for the markdown gate." >&2
  echo "Install Node.js 20+ or run the CI Markdown Quality Check instead." >&2
  exit 1
fi

# Pinned to the major used by the CI action; bump together with the action.
exec npx --yes markdownlint-cli2@0.24.0 "**/*.md" "!node_modules" "!target"
