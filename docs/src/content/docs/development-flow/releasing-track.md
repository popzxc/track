---
title: Releasing track
description: Understand the release workflow and know what artifacts each product release ships.
sidebar:
  order: 3
---

This page is for maintainers who need to publish the product release artifacts that sit on top of the repository.

## What the release workflow owns

The release pipeline has one product-facing outcome:

- create one GitHub release, `track vX.Y.Z`, for the whole product
- publish the Docker image to `ghcr.io/popzxc/track`
- attach the non-CUDA CLI archives, the shared `trackup` asset bundle, and the checksum file to that GitHub release

`release-please` owns the product version and release PR, which means frontend-only or packaging-only changes can still trigger a product release when the merged commit history says they should.

## How the workflows are structured

The repository uses three workflows instead of chaining a second workflow from the GitHub release event:

- `.github/workflows/release.yml`
  Build-checks the Docker image, prebuilds the non-CUDA CLI binaries, runs `release-please` in manifest mode, and then calls the reusable post-release workflow when a new product release is actually created.
- `.github/workflows/post-release.yml`
  Shared publication logic that verifies the GitHub release exists, normalizes its title to `track vX.Y.Z`, publishes the Docker image, and uploads the CLI archives plus the installer asset bundle.
- `.github/workflows/recover-release-assets.yml`
  Manual recovery entrypoint that reruns the same shared publication workflow against an existing release tag. Its `publish_latest` input defaults to `false` so recovery runs do not move the floating GHCR tag unless you opt in.

Both manual `workflow_dispatch` entrypoints are guarded to `main` so they cannot tag or recover from a feature branch by mistake.

The irreversible GitHub release step in `release.yml` still waits for the Dockerfile and both CLI targets to build successfully. The reusable post-release workflow then handles GHCR and release-asset publication with explicit `tag` and `version` inputs, which keeps the normal path and the recovery path on the same implementation while letting recovery rebuild from the released tag instead of whatever `main` points to later.

## Release artifacts today

Each product release currently ships:

- `track-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `track-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `trackup-assets-vX.Y.Z.tar.gz`
- `track-vX.Y.Z-sha256sums.txt`
- `ghcr.io/popzxc/track:vX.Y.Z`
- `ghcr.io/popzxc/track:latest`

The workflow intentionally omits a CUDA CLI archive for now.

TODO: add the Linux CUDA artifact after the repository has access to a GPU-capable release runner with the CUDA toolkit preinstalled.
