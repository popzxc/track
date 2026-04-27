---
title: Development Flow
description: Local contributor commands and the repo-level workflow for backend, frontend, and docs work.
sidebar:
  order: 1
---

> This section is aimed at developers and contributors. If you only want to use `track`, the earlier sections are the better place to stop.

## Tooling

For local development, keep these available:

- `just`
- Rust
- `sqlx-cli` so `cargo sqlx` is available for DAL migrations and browser e2e setup
- Bun
- Docker and `docker compose`

One reasonable install command is:

```bash
cargo install sqlx-cli --no-default-features --features sqlite,rustls
```

## Common commands

Prefer the repository `justfile` for routine task management. If a workflow in
this guide has a matching `just` recipe, use that recipe instead of manually
typing the underlying `cargo`, `bun`, or `docker` command.

From the repository root:

```bash
just test-rust
just build-rust
just install-docker
just build-all
just pr-ready
```

Frontend work:

```bash
just build-fe
just test-fe
just test-e2e
```

Docs work:

```bash
just run-docs
just build-docs
```

DAL query metadata:

```bash
just db-prepare
```

That recipe uses an ignored SQLite file at `crates/track-dal/sqlx-prepare.sqlite`
to re-run the real DAL migrations and refresh the committed offline query cache in
`crates/track-dal/.sqlx`.

Do not commit a crate-local `.env` with `DATABASE_URL` for this workflow. Normal
builds should resolve SQLx queries from the checked-in `.sqlx` cache instead of
preferring a live database connection.

## Practical workflow

For most feature work:

1. keep the backend running locally
2. register at least one project in the local UI/backend
3. use the CLI and WebUI together while iterating
4. update the Starlight docs when behavior or setup changes

If you change user-facing setup or workflow and skip the docs, the book becomes stale faster than the code.
