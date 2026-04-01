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

# Each release archive maps one uploaded Actions artifact directory to one
# published asset name. Keep the table explicit because the Linux CPU and CUDA
# builds share an OS/architecture family but are different download surfaces.
cli_asset_specs=(
  "x86_64-unknown-linux-gnu:cli-binary-x86_64-unknown-linux-gnu"
  "x86_64-unknown-linux-gnu-cuda:cli-binary-x86_64-unknown-linux-gnu-cuda"
  "aarch64-apple-darwin:cli-binary-aarch64-apple-darwin"
)

for asset_spec in "${cli_asset_specs[@]}"; do
  IFS=':' read -r asset_target artifact_name <<<"$asset_spec"

  asset_stem="track-v${TRACK_VERSION}-${asset_target}"
  asset_dir="${packages_dir}/${asset_stem}"
  source_binary="dist/raw/${artifact_name}/track"

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
