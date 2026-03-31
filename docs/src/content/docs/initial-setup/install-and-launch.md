---
title: Install and Launch
description: Install the CLI, start the local stack, and register the remote host before moving into WebUI settings.
sidebar:
  order: 2
---

Once the prerequisites are ready, the next goal is simple: get the local backend running and teach it how to reach your remote machine.

## 1. Clone the repository and install the CLI

```bash
git clone <your-track-repo-url>
cd track
cargo install --path crates/track-cli --locked
```

Make sure `~/.cargo/bin` is on your `PATH`.

## 2. Start the local API and WebUI

From the repository root:

```bash
docker compose up --build -d
```

Then open:

```text
http://localhost:3210
```

If your local user does not use UID/GID `1000:1000`, start the stack like this instead:

```bash
TRACK_UID=$(id -u) TRACK_GID=$(id -g) docker compose up --build -d
```

## 3. Keep the CLI on the default backend URL, unless you need an override

Most setups do not need a manual CLI config at all because the default backend URL is already `http://127.0.0.1:3210`.

Only run `track configure` when you want to change the backend URL or the local capture model. For example:

```bash
track configure --backend-url http://127.0.0.1:4310
track configure --model-path ~/.models/custom.gguf
```

## 4. Register the remote host and SSH key

This is the one remote-agent step that belongs in initial setup rather than in the WebUI.

```bash
track remote-agent configure \
  --host <remote-host> \
  --user <remote-user> \
  --identity-file ~/.ssh/track_remote_agent
```

Optional flags:

- `--port` defaults to `22`
- `--workspace-root` defaults to `~/workspace`
- `--projects-registry-path` defaults to `~/track-projects.json`
- `--known-hosts-file` lets you provide a prebuilt `known_hosts` file

At this stage, do **not** worry about `--shell-prelude`, review settings, or default prompts. The next section of the book handles those inside the WebUI, which keeps the guided flow much simpler.

## 5. Leave the stack running

The remaining setup chapters assume the local backend is reachable. If you shut it down, bring it back with the same `docker compose up --build -d` command.
