---
title: System Structure
description: A concise mental model of how the CLI, backend, WebUI, and remote runner fit together today.
sidebar:
  order: 2
---

> This page is for developers, contributors, and agents who need the project shape quickly.

## Repository shape

The Rust workspace is split by broad responsibility:

- `crates/track-api`
  Local HTTP API and static asset serving.
- `crates/track-cli`
  The CLI entrypoint and human-facing command surface.
- `crates/track-capture`
  Local task parsing and model resolution.
- `crates/track-config`
  Local configuration path handling.
- `crates/track-dal`
  SQLite persistence for backend-owned state.
- `crates/track-projects`
  Project registration and metadata.
- `crates/track-remote-agent`
  Remote task and PR review orchestration.
- `crates/track-types`
  Shared domain types used across crates.

The non-Rust top-level directories are:

- `frontend/`
  The Vue WebUI.
- `docs/`
  The Starlight documentation book.

## Runtime state today

The current system is centered on backend state, not on hand-edited task files.

- The CLI keeps its own config in `~/.config/track/cli.json`.
- The backend keeps live state in SQLite.
- Remote-agent SSH material is managed under backend state.

## Task capture flow

1. The CLI loads local configuration.
2. The CLI fetches registered projects from the backend.
3. The local parser turns rough prose into structured task input.
4. The backend validates and stores the task.

## Task dispatch flow

1. The WebUI asks the backend to dispatch a task.
2. The backend loads runner settings and project metadata.
3. The backend prepares or reuses remote workspace state.
4. The backend launches Codex or Claude on the remote machine.
5. The backend stores run history for the WebUI.

## PR review flow

1. The WebUI creates a review request from a GitHub PR URL.
2. The backend prepares remote review context.
3. The remote runner submits the review on GitHub.
4. The backend stores the review record and each review run.

## Deployment shape

The shipped Docker image serves both:

- `/api/*` from the Rust backend
- the built frontend assets

That is why the everyday user flow only needs one local container stack and one browser port.
