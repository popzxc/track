# AGENTS.md

This file gives future coding agents a quick working model for `track`.

## Purpose

`track` is a small local issue tracker with two user-facing workflows:

1. capture tasks quickly from the CLI
2. manage tasks in a local web UI

The system is intentionally simple. Tasks are plain Markdown files on disk, and
the rest of the codebase exists to help create, read, and mutate those files
reliably.

## How The System Is Organized

Think about the project in three layers:

- `packages/shared`
  Common schemas and types. Start here when the shape of data changes.
- `packages/core`
  Business logic and runtime behavior. Start here when capture flow, storage
  behavior, config loading, AI provider selection, or project discovery changes.
- `apps/*`
  Adapters and entrypoints:
  - CLI for task capture
  - API for JSON endpoints and static asset serving
  - Web for the browser UI

The apps should stay thin. If logic starts to matter to more than one app, it
probably belongs in `core` or `shared`.

## Important Runtime Invariants

Keep these behaviors stable unless the change is intentional:

- The filesystem is the source of truth.
- Tasks live under the configured data directory, grouped by project and then
  by status (`open` / `closed`).
- Closing and reopening a task physically moves the file between folders.
- The Markdown body is the preferred human-editable task description when
  reading a task file.
- Malformed task files should not break the whole task list; healthy tasks
  should still be visible.
- AI parsing may infer, but it must not invent arbitrary projects. The chosen
  project must come from the discovered project set.

## AI Provider Model

The AI layer is intentionally behind a small interface.

Current providers:

- OpenAI
- `llama.cpp` via `llama-cli`

Config selects the provider. Provider choice should change the integration edge,
not the business rules. Both providers should receive the same parsing contract:

- choose only from discovered projects and aliases
- normalize the description
- default priority sensibly
- fail safely when project selection is ambiguous

If you add another provider, keep the interface narrow and reuse the shared
prompt contract when possible.

## Config And Data Paths

Default locations:

- config: `~/.config/track/config.json`
- data: `~/.track/issues`

Config is shared between the CLI and API. If you change config semantics, keep
that shared contract coherent and update both docs and tests.

For `llama.cpp`, `ai.llamaCpp.llamaCliPath` may point at an absolute binary path
outside `$PATH`. `binaryPath` exists only as a backward-compatible alias.

## API And UI Relationship

The backend is not API-only in the deployed shape. It also serves built frontend
assets so the Docker image can expose one port for both the REST API and the UI.

Important implication:

- `apps/api/public/index.html` is only a placeholder/fallback
- the Docker build copies `apps/web/dist` into `apps/api/public`

Do not treat the placeholder HTML as the real frontend.

## Development Commands

From the repo root:

- `bun install`
- `bun run test`
- `bun run typecheck`
- `bun run build`

Use these as the default verification steps after meaningful changes.

## Testing Guidance

Favor small, high-signal tests.

- Prefer real filesystem tests over mocks for repository behavior.
- Mock only the AI boundary or external process boundary when needed.
- If you change storage or task parsing behavior, add or update tests in
  `packages/core`.
- If you change shared schemas, update `packages/shared` tests too.

## Documentation Style

This project prefers literate, top-down code comments in context-heavy modules.

When adding comments:

- explain why the code is structured this way
- explain tradeoffs and invariants
- place comments immediately before the block they clarify
- avoid low-signal comments that restate obvious syntax

Do not blanket the codebase with boilerplate commentary. Concentrate on the
parts where future readers would otherwise have to reconstruct intent.

## Build Artifacts And Workspace Hygiene

Do not commit local build leftovers or generated files in source directories.

In particular:

- `dist/` directories are local build artifacts
- stray generated `.js`, `.d.ts`, or `.d.ts.map` files under `src/` are usually
  accidental leftovers and should be removed
- `bun.lockb` is intentional and should stay

If you run builds locally, keep the workspace clean afterward unless the user
explicitly wants built artifacts present.

## When You Are Unsure

Choose the simpler design that keeps these properties intact:

- plain files remain easy to inspect and edit
- app layers remain thin
- AI remains replaceable
- failure modes stay understandable
- behavior is covered by a small number of meaningful tests
