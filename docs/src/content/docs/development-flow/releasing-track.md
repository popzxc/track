---
title: Releasing track
description: Understand the release workflow and know what artifacts each product release ships.
sidebar:
  order: 3
---

This page covers the current release workflow for `track`.

## Configuration

`release-please` is configured in [config.json](https://github.com/popzxc/track/blob/main/.github/release-please/config.json) and [manifest.json](https://github.com/popzxc/track/blob/main/.github/release-please/manifest.json).

The release version tracked by `release-please` lives in [Cargo.toml](https://github.com/popzxc/track/blob/main/Cargo.toml#L13). The workspace version and the internal `track-*` workspace dependency versions are marked with `# x-release-please-version`.

## Workflows

The repository currently uses these release workflows:

- [release.yml](https://github.com/popzxc/track/blob/main/.github/workflows/release.yml)
  Runs on pushes to `main` and on manual dispatch. It prebuilds the multi-architecture Docker image matrix, then runs `release-please`. When `release-please` creates a new release, this workflow calls the shared post-release workflow.
- [post-release.yml](https://github.com/popzxc/track/blob/main/.github/workflows/post-release.yml)
  Verifies the GitHub release exists, sets its title to `track vX.Y.Z`, publishes the multi-architecture Docker image to GHCR, and uploads the shared release asset bundle.
- [recover-release-assets.yml](https://github.com/popzxc/track/blob/main/.github/workflows/recover-release-assets.yml)
  Manual recovery workflow for an existing release version. It rebuilds the shared release assets from the release tag and reruns the post-release publication steps. Its `publish_latest` input defaults to `false`.

Manual dispatches for the release and recovery workflows are restricted to `main`.

## Published Artifacts

Each release currently publishes:

- a GitHub release named `track vX.Y.Z`
- `trackup-assets-vX.Y.Z.tar.gz`
- `track-vX.Y.Z-sha256sums.txt`
- `ghcr.io/popzxc/track:vX.Y.Z` as a multi-architecture manifest for `linux/amd64` and `linux/arm64`
- `ghcr.io/popzxc/track:latest` as a multi-architecture manifest for `linux/amd64` and `linux/arm64`

`trackup` installs `track` from the tagged source release with `cargo install`.
The backend wrapper keeps using those single GHCR tags, and Docker resolves the
right Linux image for the host automatically. On Linux x86_64, `trackup`
prompts for the default or CUDA-accelerated CLI build.
