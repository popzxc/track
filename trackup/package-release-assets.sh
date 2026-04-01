#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Release asset packaging
# ==============================================================================
#
# The release and recovery workflows both need the same product artifact set.
# Keeping the packaging logic here prevents the two workflows from drifting in
# file names, bundle contents, or checksum coverage.

TRACK_VERSION="${TRACK_VERSION:?TRACK_VERSION must be set}"

packages_dir="dist/packages"
mkdir -p "$packages_dir"

for target in x86_64-unknown-linux-gnu aarch64-apple-darwin; do
  asset_stem="track-v${TRACK_VERSION}-${target}"
  asset_dir="${packages_dir}/${asset_stem}"
  source_binary="dist/raw/cli-binary-${target}/track"

  mkdir -p "$asset_dir"
  cp "$source_binary" "${asset_dir}/track"
  chmod +x "${asset_dir}/track"
  cp README.md LICENSE "${asset_dir}/"
  tar -C "$packages_dir" -czf "dist/${asset_stem}.tar.gz" "${asset_stem}"
done

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

sha256sum dist/*.tar.gz > "dist/track-v${TRACK_VERSION}-sha256sums.txt"
