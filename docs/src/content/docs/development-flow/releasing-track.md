---
title: Releasing track
description: Understand the release workflow and know what artifacts each product release ships.
sidebar:
  order: 3
---

This page covers the current release workflow for `track`.

## Configuration

`release-please` is configured in [config.json](/home/popzxc/workspace/airbender/track/.github/release-please/config.json) and [manifest.json](/home/popzxc/workspace/airbender/track/.github/release-please/manifest.json).

The release version tracked by `release-please` lives in [Cargo.toml](/home/popzxc/workspace/airbender/track/Cargo.toml#L13). The workspace version and the internal `track-*` workspace dependency versions are marked with `# x-release-please-version`.

## Workflows

The repository currently uses these release workflows:

- [release.yml](/home/popzxc/workspace/airbender/track/.github/workflows/release.yml)
  Runs on pushes to `main` and on manual dispatch. It prebuilds the Docker image, the non-CUDA CLI binaries, and the Linux CUDA CLI binary, then runs `release-please`. When `release-please` creates a new release, this workflow calls the shared post-release workflow.
- [post-release.yml](/home/popzxc/workspace/airbender/track/.github/workflows/post-release.yml)
  Verifies the GitHub release exists, sets its title to `track vX.Y.Z`, publishes the Docker image to GHCR, and uploads the release assets. Recovery runs rebuild the same CPU and CUDA CLI binaries from the tagged source.
- [recover-release-assets.yml](/home/popzxc/workspace/airbender/track/.github/workflows/recover-release-assets.yml)
  Manual recovery workflow for an existing release version. It rebuilds from the release tag and reruns the post-release publication steps. Its `publish_latest` input defaults to `false`.

Manual dispatches for the release and recovery workflows are restricted to `main`.

## Published Artifacts

Each release currently publishes:

- a GitHub release named `track vX.Y.Z`
- `track-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `track-vX.Y.Z-x86_64-unknown-linux-gnu-cuda.tar.gz`
- `track-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `trackup-assets-vX.Y.Z.tar.gz`
- `track-vX.Y.Z-sha256sums.txt`
- `ghcr.io/popzxc/track:vX.Y.Z`
- `ghcr.io/popzxc/track:latest`

The current workflow publishes CLI binaries for Linux x86_64, Linux x86_64 CUDA, and Apple Silicon macOS. The CUDA artifact is built in a CUDA toolkit container and is not executed during the release workflow.

`trackup` installs the default portable CLI unless the user passes `--cuda`, in which case it selects the Linux x86_64 CUDA asset.
