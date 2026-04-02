#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Release asset packaging
# ==============================================================================
#
# The release and recovery workflows both publish the same backend-side asset
# bundle. `trackup` installs the CLI from the tagged source release, so the
# GitHub release only needs to ship the wrapper scripts and the pinned Compose
# file that match the backend image tag.
#
# The smoke installer reuses this same packaging path for arbitrary git refs,
# but swaps in a caller-provided local image tag instead of a published GHCR
# tag. Keeping both paths on one script avoids drifting asset layouts.

TRACK_VERSION="${TRACK_VERSION:?TRACK_VERSION must be set}"
TRACK_ASSET_LABEL="${TRACK_ASSET_LABEL:-v${TRACK_VERSION}}"
TRACK_IMAGE_REF="${TRACK_IMAGE_REF:-ghcr.io/popzxc/track:v${TRACK_VERSION}}"

packages_dir="dist/packages"
mkdir -p "$packages_dir"

shared_asset_stem="trackup-assets-${TRACK_ASSET_LABEL}"
shared_asset_dir="${packages_dir}/${shared_asset_stem}"
mkdir -p "$shared_asset_dir"

cp trackup/trackup "${shared_asset_dir}/trackup"
chmod +x "${shared_asset_dir}/trackup"
cp trackup/track-backend "${shared_asset_dir}/track-backend"
chmod +x "${shared_asset_dir}/track-backend"
sed "s|__TRACK_IMAGE_REF__|${TRACK_IMAGE_REF}|g" \
  trackup/track-backend.compose.yaml.in \
  > "${shared_asset_dir}/track-backend.compose.yaml"

tar -C "$packages_dir" -czf "dist/${shared_asset_stem}.tar.gz" "${shared_asset_stem}"

sha256sum "dist/${shared_asset_stem}.tar.gz" > "dist/track-v${TRACK_VERSION}-sha256sums.txt"
