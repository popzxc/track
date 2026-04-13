---
title: Configuration Reference
description: Look up the exact fields, defaults, and where each setting is edited today.
sidebar:
  order: 3
---

## CLI config fields

These live in `~/.config/track/cli.json`.

| Field | Default | Meaning |
| --- | --- | --- |
| `backendBaseUrl` | `http://127.0.0.1:3210` | Which local backend the CLI talks to. |
| `llamaCpp.modelPath` | unset | Absolute or relative path to a local GGUF file you manage yourself. |
| `llamaCpp.modelHfRepo` | `bartowski/Meta-Llama-3-8B-Instruct-GGUF` | Hugging Face repository for the default managed model. |
| `llamaCpp.modelHfFile` | `Meta-Llama-3-8B-Instruct-Q4_K_M.gguf` | Hugging Face file name for the default managed model. |

`modelHfRepo` and `modelHfFile` must be set together.

## Remote agent registration fields

These are initially set by `track remote-agent configure`, then stored in backend state.

| Field | Default | Meaning |
| --- | --- | --- |
| `host` | none | Remote host name or IP address. |
| `user` | none | Remote SSH user. |
| `port` | `22` | Remote SSH port. |
| `workspaceRoot` | `~/workspace` | Root directory on the remote machine where reusable checkouts live. |
| `projectsRegistryPath` | `~/track-projects.json` | JSON registry file the backend uses to keep track of prepared remote projects. |

## WebUI runner settings

These are edited in **Settings → Runner setup** and also stored in backend state.

| Field | Default | Meaning |
| --- | --- | --- |
| `preferredTool` | `codex` | Default runner for new task dispatches and new review requests. |
| `shellPrelude` | empty | Shell snippet that runs before every remote command. |
| `reviewFollowUp.enabled` | `false` | Enables automatic review follow-up behavior. |
| `reviewFollowUp.mainUser` | empty | Main GitHub user name used for PR review flows. |
| `reviewFollowUp.defaultReviewPrompt` | empty | Reusable review guidance appended to manual review requests. |

## Environment variables worth knowing

| Variable | Meaning |
| --- | --- |
| `TRACK_CLI_CONFIG_PATH` | Overrides the location of `cli.json`. |
| `TRACK_STATE_DIR` | Overrides the backend state directory when running the backend directly. |
| `TRACK_WEB_BIND_HOST` | Changes which host interface `docker compose` publishes. The default is `127.0.0.1`. |
| `TRACK_WEB_PORT` | Changes the host port published by `docker compose`. |
| `TRACK_UID` / `TRACK_GID` | Adjust the Docker image user to match your host UID/GID. |

`TRACK_WEB_BIND_HOST=0.0.0.0` intentionally exposes the unauthenticated backend
beyond localhost. Use that only when you are also providing your own access
controls.
