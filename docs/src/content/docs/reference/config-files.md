---
title: Config Files and State
description: See where track stores its live configuration and how that differs from older setups.
sidebar:
  order: 1
---

This page is for lookup, not onboarding. If you are still setting things up for the first time, go back to the guided chapters.

## Current live state

| Location | Purpose |
| --- | --- |
| `~/.config/track/cli.json` | CLI-side settings such as the backend URL and local model override. |
| backend state directory | Stores the live SQLite database plus managed remote-agent secrets. |
| backend state `track.sqlite` | Registered projects, tasks, runs, reviews, and settings. |
| backend state `remote-agent/id_ed25519` | Managed SSH private key used by the backend for remote work. |
| backend state `remote-agent/known_hosts` | Managed `known_hosts` file for remote SSH calls. |

## Where the backend state lives

When you run the shipped Docker Compose setup, the backend state bind-mounts from:

```text
${HOME}/.track/backend
```

Compose creates that host directory if it is missing.

When you run the backend outside Docker, the default state directory is:

```text
~/.track/backend
```

You can override that with `TRACK_STATE_DIR`.

## CLI config example

The smallest useful `~/.config/track/cli.json` file looks like this:

```json
{
  "backendBaseUrl": "http://127.0.0.1:3210"
}
```

With a custom local model override:

```json
{
  "backendBaseUrl": "http://127.0.0.1:3210",
  "llamaCpp": {
    "modelPath": "/home/user/.models/custom.gguf"
  }
}
```
