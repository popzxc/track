---
title: Local Prerequisites
description: Prepare the machine where you will run the track CLI and open the local WebUI.
sidebar:
  order: 3
---

This page is about your local machine: the one where you run `track`, capture tasks, and open the browser UI.

## Rust toolchain

You need Rust locally to build and install `track`.

Install it with:

```bash
curl https://sh.rustup.rs -sSf | sh
. "$HOME/.cargo/env"
```

After that, make sure `~/.cargo/bin` is on your `PATH`.

## Native build prerequisites

`track-cli` builds the local `llama.cpp` capture backend through Rust bindings, so your machine needs the normal native build tooling plus `libclang`.

On Debian or Ubuntu, a good baseline is:

```bash
sudo apt update
sudo apt install -y build-essential cmake clang libclang-dev pkg-config
```

If you use another distro or macOS, install the equivalent C/C++ toolchain, CMake, Clang, and `libclang` package for your platform.

## Docker and Docker Compose

You also need Docker with `docker compose` on the local machine so you can run the local API and WebUI.

Verify that part before continuing:

```bash
docker compose version
```

## Local capture model

`track` uses a local GGUF model for task capture.

For the normal setup path, you do not need to prepare a model manually. The first task capture downloads the default model into `~/.track/models` automatically if it is not already cached.

If you know in advance that you want a different model, keep the GGUF file at a stable path and override it later through `track configure --model-path ...` or the configuration file. That is an advanced setup, not something you need before the first run.
