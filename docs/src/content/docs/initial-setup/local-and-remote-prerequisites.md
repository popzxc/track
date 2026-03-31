---
title: Local and Remote Prerequisites
description: Prepare the machines and accounts you need before touching the WebUI.
sidebar:
  order: 1
---

This chapter gets the environment ready without asking you to configure anything inside the WebUI yet.

## Local machine

You need the following on the machine where you will run `track` and open the browser UI:

- Rust, so you can install the CLI.
- Native build tools for the local `llama.cpp` capture backend.
- Docker with `docker compose`, so you can run the local API and WebUI.

On Debian or Ubuntu, a good baseline is:

```bash
sudo apt update
sudo apt install -y build-essential cmake clang libclang-dev pkg-config
```

## Remote machine

You also need a Linux machine that can act as the remote runner. It should already have:

- `git`
- `gh`
- either `codex`, `claude`, or both

It should also be able to:

- clone your repositories
- push branches
- open pull requests on GitHub

Using a dedicated GitHub automation account is recommended, but not required by the software itself.

## SSH access

Create a dedicated SSH key for `track` on your local machine:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/track_remote_agent -C "track remote agent"
```

Install the public key on the remote machine and make sure login works:

```bash
ssh-copy-id -i ~/.ssh/track_remote_agent.pub <remote-user>@<remote-host>
ssh -i ~/.ssh/track_remote_agent <remote-user>@<remote-host>
```

## Quiet shell setup on the remote machine

Later, the WebUI will ask for a shell prelude. Before you get there, make sure you know which lines on the remote machine are responsible for setting up your toolchain.

Typical examples are:

```bash
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
. "$HOME/.cargo/env"
export PATH="$PATH:/home/<your-user>/.foundry/bin"
```

Keep that snippet quiet on stdout. It should set environment variables and PATH entries, but it should not print banners, prompts, or status messages.
