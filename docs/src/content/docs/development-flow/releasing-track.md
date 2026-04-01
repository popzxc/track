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
  Runs on pushes to `main` and on manual dispatch. It prebuilds the Docker image, then runs `release-please`. When `release-please` creates a new release, this workflow calls the shared post-release workflow.
- [post-release.yml](/home/popzxc/workspace/airbender/track/.github/workflows/post-release.yml)
  Verifies the GitHub release exists, sets its title to `track vX.Y.Z`, publishes the Docker image to GHCR, and uploads the shared release asset bundle.
- [recover-release-assets.yml](/home/popzxc/workspace/airbender/track/.github/workflows/recover-release-assets.yml)
  Manual recovery workflow for an existing release version. It rebuilds the shared release assets from the release tag and reruns the post-release publication steps. Its `publish_latest` input defaults to `false`.

Manual dispatches for the release and recovery workflows are restricted to `main`.

## Published Artifacts

Each release currently publishes:

- a GitHub release named `track vX.Y.Z`
- `trackup-assets-vX.Y.Z.tar.gz`
- `track-vX.Y.Z-sha256sums.txt`
- `ghcr.io/popzxc/track:vX.Y.Z`
- `ghcr.io/popzxc/track:latest`

`trackup` installs `track` from the tagged source release with `cargo install`.
On Linux x86_64, it prompts for the default or CUDA-accelerated CLI build.
