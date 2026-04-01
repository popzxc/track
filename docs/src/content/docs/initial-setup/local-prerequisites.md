---
title: Local Prerequisites
description: Prepare the machine where you will run the track CLI and open the local WebUI.
sidebar:
  order: 3
---

This page is about your local machine: the one where you run `track`, capture tasks, and open the browser UI.

## Release installer prerequisites

The normal install path uses the released `trackup` bootstrap script rather
than a local source build.

Make sure these commands exist locally before you continue:

```bash
curl --version
jq --version
tar --version
docker compose version
```

`trackup` also verifies release checksums before it installs anything. It can
use either `sha256sum` or `shasum`, and most Linux and macOS systems already
ship one of those by default.

Released installers currently support Linux x86_64 and macOS arm64.

## Building from source is optional

You only need Rust, a native build toolchain, and `libclang` if you plan to
build `track` from a repository checkout instead of using the released
installer.

## Local capture model

`track` uses a local GGUF model for task capture.

For the normal setup path, you do not need to prepare a model manually. The first task capture downloads the default model into `~/.track/models` automatically if it is not already cached.

If you know in advance that you want a different model, keep the GGUF file at a stable path and override it later through `track configure --model-path ...` or the configuration file. That is an advanced setup, not something you need before the first run.
