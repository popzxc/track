---
title: System Structure
description: A concise mental model of how the CLI, backend, WebUI, and remote runner fit together today.
sidebar:
  order: 2
---

> This page is for developers, contributors, and agents who need the project shape quickly.

## Top-level modules

- `crates/track-core`
  Shared types, repositories, backend settings, and remote-agent orchestration.
- `crates/track-capture`
  Local capture parsing and model resolution.
- `crates/track-cli`
  The CLI entrypoint and human-facing command surface.
- `crates/track-api`
  The Axum backend that serves JSON endpoints and the built frontend.
- `frontend/`
  The Vue WebUI.
- `docs/`
  The Starlight documentation book.

## Runtime state today

The current system is centered on backend state, not on hand-edited task files.

- the CLI keeps its own config in `~/.config/track/cli.json`
- the backend keeps live state in SQLite
- remote-agent SSH material is managed under backend state

## Task capture flow

1. the CLI loads `cli.json`
2. the CLI fetches registered projects from the backend
3. the local parser turns rough prose into structured task input
4. the backend validates and stores the task

## Task dispatch flow

1. the WebUI asks the backend to dispatch a task
2. the backend loads remote-agent settings and project metadata
3. the backend prepares or reuses a remote checkout and worktree
4. the backend launches Codex or Claude on the remote machine
5. the backend persists run history for the WebUI

## PR review flow

1. the WebUI creates a review request from a GitHub PR URL
2. the backend prepares remote review context
3. the remote runner submits the review on GitHub
4. the backend stores the review record and every review run

## Deployment shape

The shipped Docker image serves both:

- `/api/*` from the Rust backend
- the built frontend assets

That is why the everyday user flow only needs one local container stack and one browser port.
