#!/usr/bin/env bash
# ==============================================================================
# Launcher Icon Generator (moggyt)
#
# Derives every hicolor icon size from the 1024x1024 design master
# (packaging/icons/moggyt-1024.png) by downscaling with a Lanczos filter.
#
# Usage:
#   ./scripts/generate-icons.sh
# ==============================================================================

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
SOURCE_ICON="${REPO_ROOT}/packaging/icons/moggyt-1024.png"
ICON_ROOT="${REPO_ROOT}/packaging/icons/hicolor"
SIZES=(16 32 48 64 128 256)

command -v magick >/dev/null 2>&1 || {
  echo "Error: ImageMagick (magick) is required." >&2
  exit 1
}

for size in "${SIZES[@]}"; do
  target_dir="${ICON_ROOT}/${size}x${size}/apps"
  mkdir -p "${target_dir}"
  magick "${SOURCE_ICON}" -strip -filter Lanczos -resize "${size}x${size}" \
    "${target_dir}/moggyt.png"
  echo "Wrote ${target_dir}/moggyt.png"
done
