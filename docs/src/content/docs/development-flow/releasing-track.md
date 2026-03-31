---
title: Releasing track
description: Bootstrap crates.io publishing, understand the release workflow, and know what artifacts each product release ships.
sidebar:
  order: 3
---

This page is for maintainers who need to publish the shared Rust workspace and the product artifacts that sit on top of it.

## What the release workflow owns

The release pipeline has one product-facing outcome:

- publish `track-core`, `track-capture`, `track-cli`, and `track-api` to crates.io
- create one GitHub release, `track vX.Y.Z`, from `track-cli`
- publish the Docker image to `ghcr.io/popzxc/track`
- attach the non-CUDA CLI archives, the shared `trackup` asset bundle, and the checksum file to that GitHub release

That split keeps crates.io in sync with the workspace while still treating the CLI release as the public product release that users download.

## First-time bootstrap

Trusted publishing on crates.io only works after each crate has been published once.

Before relying on the GitHub Actions workflow:

1. publish `track-core`, `track-capture`, `track-cli`, and `track-api` manually the first time
2. enable crates.io trusted publishing for those four crates
3. let the `Release` workflow publish future versions through GitHub OIDC

The quickest safety check before that first publish is:

```bash
cargo publish --dry-run -p track-core
cargo publish --dry-run -p track-capture
cargo publish --dry-run -p track-cli
cargo publish --dry-run -p track-api
```

## How the workflows are structured

The repository uses three workflows instead of chaining a second workflow from the GitHub release event:

- `.github/workflows/release.yml`
  Opens or updates the release PR, build-checks the Docker image, prebuilds the non-CUDA CLI binaries, publishes the workspace with `release-plz`, and then calls the reusable post-release workflow.
- `.github/workflows/post-release.yml`
  Shared post-release logic that verifies the GitHub release exists, publishes the Docker image, and uploads the CLI archives plus the installer asset bundle.
- `.github/workflows/recover-release-assets.yml`
  Manual recovery entrypoint that rebuilds the Docker image and CLI binaries from an existing release tag, then calls the same shared post-release workflow. Its `publish_latest` input defaults to `false` so recovery runs do not move the floating GHCR tag unless you opt in.

Both manual `workflow_dispatch` entrypoints are guarded to `main` so they cannot tag or recover from a feature branch by mistake.

The irreversible publish step in `release.yml` still waits for the Dockerfile and both CLI targets to build successfully. The reusable post-release workflow then handles the release-coupled work with explicit `tag` and `version` inputs, which keeps the normal path and the recovery path on the same implementation while letting recovery rebuild from the released tag instead of whatever `main` points to later.

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
