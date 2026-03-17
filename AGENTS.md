# AGENTS.md

This file gives future coding agents a quick working model for `track`.

## Purpose

`track` is a small local issue tracker with two user-facing workflows:

1. capture tasks quickly from the Rust CLI
2. manage tasks in a local web UI

Tasks are plain Markdown files on disk. The rest of the codebase exists to help
create, read, and mutate those files reliably.

## How The System Is Organized

Think about the project as one Rust backend split into crates plus one frontend.

- `crates/track-core`
  Shared backend behavior. Start here when config loading, project discovery,
  task storage, sorting, or `llama-completion` integration changes.
- `crates/track-cli`
  CLI entrypoint and user-facing capture output.
- `crates/track-api`
  Axum routes, HTTP error mapping, and static frontend serving.
- `frontend/`
  Vue/Vite UI only.

The stable shared contract is:

- `~/.config/track/config.json`
- Markdown task files under `~/.track/issues`
- the JSON API exposed by `track-api`

If you change those contracts, update every layer that depends on them.

## Important Runtime Invariants

Keep these behaviors stable unless the change is intentional:

- The filesystem is the source of truth.
- Tasks live under the configured data directory, grouped by project and then
  by status (`open` / `closed`).
- A task's identity comes from its file path, not duplicated YAML fields.
- Closing and reopening a task physically moves the file between folders.
- The Markdown body is the preferred human-editable task description when
  reading a task file.
- Malformed task files should not break the whole task list; healthy tasks
  should still be visible.
- AI parsing may infer, but it must not invent arbitrary projects. The chosen
  project must come from the discovered project set.

## Local Model Contract

The backend supports local parsing only.

- `llama.cpp` is invoked through the `llama-completion` binary.
- `llamaCpp.modelPath` is required in config.
- `llamaCpp.llamaCompletionPath` is optional; if it is absent, the CLI uses
  `llama-completion` from `$PATH`.

Do not reintroduce hosted-model assumptions without an explicit user request.

## API And Frontend Relationship

`track-api` is not API-only in the deployed shape. It also serves the built
frontend assets so Docker can expose one port for both the REST API and the UI.

Important implication:

- frontend development stays separate under `frontend/`
- production serving happens from `track-api`
- Docker copies `frontend/dist` into the runtime image and `track-api` serves it

## Development Commands

From the repo root:

- `cargo test --workspace`
- `cargo build --release -p track-cli`
- `cargo build --release -p track-api`
- `cargo run -p track-api`
- `cd frontend && bun install`
- `cd frontend && bun run dev`
- `cd frontend && bun run typecheck`
- `cd frontend && bun run build`

Use Cargo for backend work. Use Bun only inside `frontend/`.

## Testing Guidance

Favor small, high-signal tests.

- Prefer real filesystem tests over mocks for repository behavior.
- Mock only the external process boundary when needed, or use tiny fake
  `llama-completion` scripts in temp directories.
- If you change config shape, capture behavior, or storage semantics,
  update Rust tests in `track-core` or `track-cli`.
- If you change the API surface, add or update Rust HTTP tests in `track-api`.
- If you change the frontend contract, keep the frontend types aligned with the
  Rust API responses.

## Documentation Style

This project prefers literate, top-down code comments in context-heavy modules.

When adding comments:

- explain why the code is structured this way
- explain tradeoffs and invariants
- place comments immediately before the block they clarify
- avoid low-signal comments that restate obvious syntax

Concentrate comments where future readers would otherwise have to reconstruct
intent.

## Build Artifacts And Workspace Hygiene

Do not commit local build leftovers or generated files in source directories.

In particular:

- `target/` and `dist/` directories are local build artifacts
- stray generated `.js`, `.d.ts`, or `.d.ts.map` files under source directories
  are accidental leftovers and should be removed
- `Cargo.lock` is intentional and should stay
- Bun lockfiles, if present under `frontend/`, are intentional and should stay
