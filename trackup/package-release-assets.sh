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

TRACK_VERSION="${TRACK_VERSION:?TRACK_VERSION must be set}"

packages_dir="dist/packages"
mkdir -p "$packages_dir"

shared_asset_stem="trackup-assets-v${TRACK_VERSION}"
shared_asset_dir="${packages_dir}/${shared_asset_stem}"
mkdir -p "$shared_asset_dir"

cp trackup/trackup "${shared_asset_dir}/trackup"
chmod +x "${shared_asset_dir}/trackup"
cp trackup/track-backend "${shared_asset_dir}/track-backend"
chmod +x "${shared_asset_dir}/track-backend"
sed "s/__TRACK_VERSION__/v${TRACK_VERSION}/g" \
  trackup/track-backend.compose.yaml.in \
  > "${shared_asset_dir}/track-backend.compose.yaml"

tar -C "$packages_dir" -czf "dist/${shared_asset_stem}.tar.gz" "${shared_asset_stem}"

sha256sum "dist/${shared_asset_stem}.tar.gz" > "dist/track-v${TRACK_VERSION}-sha256sums.txt"
