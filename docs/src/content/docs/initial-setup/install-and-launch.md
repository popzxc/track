---
title: Install and Launch
description: Install the CLI, start the local stack, and register the remote host after both setup machines are ready.
sidebar:
  order: 5
---

Once you have worked through [Intro](../intro/), [Security](../security/), [Local Prerequisites](../local-prerequisites/), and [Remote Prerequisites](../remote-prerequisites/), the next goal is simple: get the local backend running and teach it how to reach your remote machine.

## 1. Install the release bundle

```bash
curl -fsSL https://raw.githubusercontent.com/popzxc/track/main/trackup/trackup | bash
```

The installer downloads a matched GitHub release, puts `track`, `trackup`, and
`track-backend` into `~/.track/bin`, writes the shipped backend Compose file
into `~/.track/share`, builds `track` from the tagged source release, and
prompts you to reload your shell if it had to add `~/.track/bin` to your
`PATH`.

This installer now expects local prerequisites for both the release download
and the tagged-source CLI build, including `git`, `jq`, a Rust toolchain,
`cmake`, `clang`, and a C/C++ compiler.

On Linux x86_64, `trackup` asks whether to install the default CLI build or a
CUDA-accelerated build. The CUDA option requires a local CUDA toolkit
installation and is only offered on Linux x86_64.

Re-run `trackup` later to update to the newest release. Use `trackup vX.Y.Z`
when you need to pin a specific release.

## 2. Start the local API and WebUI

```bash
track-backend up -d
```

Then open:

```text
http://localhost:3210
```

`track-backend` forwards to the installed release Compose file, exports your
current UID/GID, and auto-detects whether your machine is using Docker or
Podman. On rootless Podman it adds the `keep-id` user-namespace override so
the bind-mounted backend state directory stays writable without requiring a
local image build.

By default the shipped Compose file binds the backend to `127.0.0.1` only.
That is intentional because `track` does not provide a login or authorization
flow.

If you deliberately want LAN or host-network exposure, opt into it yourself:

```bash
TRACK_WEB_BIND_HOST=0.0.0.0 track-backend up -d
```

If you do that, it is your responsibility to place the service behind your own
network controls, reverse proxy policy, VPN, firewall rules, or other access
restrictions.

## 3. Keep the CLI on the default backend URL, unless you need an override

Most setups do not need a manual CLI config at all because the default backend URL is already `http://127.0.0.1:3210`.

Only run `track configure` when you want to change the backend URL or the local capture model. For example:

```bash
track configure --backend-url http://127.0.0.1:4310
track configure --model-path ~/.models/custom.gguf
```

## 4. Register the remote host and import the dedicated SSH key

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

If you followed the remote prerequisites page, `~/.ssh/track_remote_agent` is the dedicated key you created earlier. `track` imports it into its managed automation directory, which is why that key must be dedicated to this workflow.

At this stage, do **not** worry about `--shell-prelude`, review settings, or default prompts. The next section of the book handles those inside the WebUI, which keeps the guided flow much simpler.

## 5. Leave the stack running

The remaining setup chapters assume the local backend is reachable. If you shut
it down, bring it back with `track-backend up -d`.
