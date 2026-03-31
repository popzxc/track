---
title: CLI Reference
description: Look up the current command surface for capture, configuration, project registration, and migration.
sidebar:
  order: 2
---

All commands below assume the local backend is already reachable at the configured `backendBaseUrl`.

## `track <free-form task>`

Creates a task from rough prose.

Example:

```bash
track todoapp prio medium add a safer fallback when remote cleanup fails
```

Notes:

- the project must already be registered
- the local parser runs in the CLI
- the task itself is created through the backend API

## `track configure`

Updates the CLI config file at `~/.config/track/cli.json`.

Options:

- `--backend-url <url>`
- `--model-path <path>`
- `--model-hf-repo <repo>`
- `--model-hf-file <file>`

Examples:

```bash
track configure --backend-url http://127.0.0.1:4310
track configure --model-path ~/.models/parser.gguf
track configure --model-hf-repo bartowski/Meta-Llama-3-8B-Instruct-GGUF --model-hf-file Meta-Llama-3-8B-Instruct-Q4_K_M.gguf
```

## `track project register`

Registers a local Git checkout as a destination project for task capture.

Usage:

```bash
track project register [path] [--alias <alias>...]
```

Examples:

```bash
track project register
track project register --alias todoapp --alias todo
track project register ~/workspace/project-a --alias payments
```

## `track remote-agent configure`

Registers the remote host and uploads the SSH material the backend will use.

Required options:

- `--host <host>`
- `--user <user>`
- `--identity-file <path>`

Common optional options:

- `--port <port>`
- `--workspace-root <path>`
- `--projects-registry-path <path>`
- `--known-hosts-file <path>`
- `--shell-prelude <text>`
- `--shell-prelude-file <path>`
- `--enable-review-follow-up`
- `--main-user <github-user>`
- `--default-review-prompt <text>`
- `--default-review-prompt-file <path>`

Minimal example:

```bash
track remote-agent configure \
  --host runner.example.com \
  --user builder \
  --identity-file ~/.ssh/track_remote_agent
```

## `track migrate status`

Shows whether a legacy install has data that can be imported into the current SQLite backend.

## `track migrate import`

Imports legacy data into the current backend.

Use this only when migrating an older install. New setups do not need it.

## No-argument behavior

Running plain `track` with no subcommand behaves like `track configure`.
