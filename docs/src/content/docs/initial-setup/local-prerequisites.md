---
title: Local Prerequisites
description: Prepare the machine where you will run the track CLI and open the local WebUI.
sidebar:
  order: 3
---

This page is about your local machine: the one where you run `track`, capture tasks, and open the browser UI.

## Release installer prerequisites

The normal install path uses the released `trackup` bootstrap script. That
script downloads the shared backend assets from the GitHub release and builds
the `track` CLI from the tagged source release with `cargo install`.

Make sure these tools exist locally before you continue:

- `git`
- `curl`
- `rustc`
- `cargo`
- `jq`
- `tar`
- `cmake`
- `clang`
- `docker compose`
- one C compiler: `cc`, `gcc`, or `clang`
- one C++ compiler: `c++`, `g++`, or `clang++`

If you want a quick verification pass, this is a reasonable checklist:

```bash
git --version
curl --version
rustc --version
cargo --version
jq --version
tar --version
cmake --version
clang --version
docker compose version
command -v cc || command -v gcc || command -v clang
command -v c++ || command -v g++ || command -v clang++
```

`trackup` also verifies release checksums before it installs anything. It can
use either `sha256sum` or `shasum`, and most Linux and macOS systems already
ship one of those by default.

On Linux x86_64, `trackup` can also install a CUDA-accelerated CLI build. That
path requires a local CUDA toolkit installation with `nvcc` available.

## Local capture model

`track` uses a local GGUF model for task capture.

For the normal setup path, you do not need to prepare a model manually. The first task capture downloads the default model into `~/.track/models` automatically if it is not already cached.

If you know in advance that you want a different model, keep the GGUF file at a stable path and override it later through `track configure --model-path ...` or the configuration file. That is an advanced setup, not something you need before the first run.
