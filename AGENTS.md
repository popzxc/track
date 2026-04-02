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
  Shared backend behavior. Start here when config loading, CLI-side project
  discovery, project metadata persistence, task storage, sorting, or remote
  agent behavior changes.
- `crates/track-capture`
  CLI capture and local-model parsing. Start here when prompt shaping,
  model download, or `llama.cpp` binding integration changes.
- `crates/track-cli`
  CLI entrypoint and user-facing capture output.
- `crates/track-api`
  Axum routes, HTTP error mapping, and static frontend serving.
- `crates/track-integration-tests`
  Live SSH-backed integration tests for remote dispatch flows. Use this when
  you need real `ssh`/`scp` coverage with mocked `gh` and `codex`.
- `frontend/`
  Vue/Vite UI only.
- `testing/`
  Shared test infrastructure, including the Docker SSH fixture and mock CLI
  implementations used by the live integration tests.

The stable shared contract is:

- `~/.config/track/config.json`
- Markdown task files under `~/.track/issues`
- project metadata files at `~/.track/issues/<project>/PROJECT.md`
- local dispatch records under `~/.track/issues/.dispatches`
- the JSON API exposed by `track-api`

If you change those contracts, update every layer that depends on them.

## Important Runtime Invariants

Keep these behaviors stable unless the change is intentional:

- The filesystem is the source of truth.
- Tasks live under the configured data directory, grouped by project and then
  by status (`open` / `closed`).
- A task's identity comes from its file path, not duplicated YAML fields.
- Project metadata lives in `project/PROJECT.md` under the track data
  directory.
- The CLI initializes `PROJECT.md` because it can see host repositories.
- The API and frontend list projects from the track data directory only.
- Managed remote-agent SSH material lives under `~/.track/remote-agent`.
- Hidden implementation directories under the track data root must not appear
  as user projects.
- Closing and reopening a task physically moves the file between folders.
- The Markdown body is the preferred human-editable task description when
  reading a task file.
- New CLI-created task bodies separate `## Summary` from `## Original note`
  so local and remote automation can tell normalized context apart from the
  raw user note.
- Malformed task files should not break the whole task list; healthy tasks
  should still be visible.
- AI parsing may infer, but it must not invent arbitrary projects. The chosen
  project must come from the discovered project set.

## Local Model Contract

The backend supports local parsing only.

- Capture uses in-process `llama.cpp` Rust bindings.
- If `llamaCpp` is empty or missing model overrides, `track` uses the built-in
  default Hugging Face model settings.
- Config may provide either `llamaCpp.modelPath` or both
  `llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile`.
- When Hugging Face config is active, the model is cached under
  `~/.track/models`.
- On supported Linux hosts with NVIDIA GPUs, the CUDA-enabled CLI build is the
  recommended local parsing path because it materially improves capture
  latency. The CPU build remains the portable fallback.

Do not reintroduce hosted-model assumptions without an explicit user request.

## Local API Contract

- `api.port` controls where the CLI looks for the local API.
- After CLI task creation, `track` sends a best-effort local notify call.
- The frontend watches the API's task-change version so CLI-created tasks
  appear automatically when the API is running.
- The frontend also polls dispatch state for remote Codex runs.

## Remote Agent Contract

- `remoteAgent` in config is optional.
- The config wizard copies the imported SSH key into a managed path under
  `~/.track/remote-agent` instead of depending on `~/.ssh`.
- `track-api` uses the system `ssh` and `scp` clients to communicate with the
  remote machine.
- `remoteAgent.shellPrelude` is a user-managed shell snippet that runs before
  remote SSH commands so PATH/toolchain setup does not depend on interactive
  shell startup.
- The remote host is expected to already have `git`, `gh`, and `codex`.
- The remote projects registry is JSON, not Markdown, because the automation
  owns that file and benefits from deterministic parsing.
- The remote prompt is uploaded as a file and piped into `codex exec` through
  stdin. Do not switch back to long shell-escaped prompt arguments without a
  strong reason.

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
- `RUN_TRACK_INTEGRATION_TESTS=true cargo test -p track-integration-tests --test remote_dispatch -- --nocapture`
- `cd frontend && bun install`
- `cd frontend && bun run dev`
- `cd frontend && bun run typecheck`
- `cd frontend && bun run build`

Use Cargo for backend work. Use Bun only inside `frontend/`.

## Testing Guidance

Favor small, high-signal tests.

- Prefer real filesystem tests over mocks for repository behavior.
- For CLI capture tests, prefer injecting fake parser results over trying to
  emulate a real local model.
- If you change config shape, capture behavior, or storage semantics,
  update Rust tests in `track-core` or `track-cli`.
- Every new SQLite migration must include a dedicated migration test under
  `crates/track-core/src/database/migration_tests/` in its own numbered file,
  for example `id_000_<migration_name>.rs`.
- Each migration test must create the database in the immediately
  pre-migration schema state, populate representative rows, run the real
  `DatabaseContext::initialize()` path, and assert that the migration keeps the
  rows intact without corrupting their data.
- If you change the API surface, add or update Rust HTTP tests in `track-api`.
- For remote-agent behavior that depends on real `ssh`/`scp`, prefer the live
  tests in `crates/track-integration-tests` over trying to mock the transport.
- The live integration tests require Docker plus
  `RUN_TRACK_INTEGRATION_TESTS=true`. Without that env var they print a skip
  message and exit successfully.
- These live tests are intentionally expensive. Add or expand them only for
  high-signal end-to-end flows where real remote behavior matters, such as
  dispatch launch, follow-up reuse, or concurrent dispatch tracking.
- Do not add live integration tests for minor behavior that is already covered
  by unit tests or in-process API tests.
- Before adding a new live integration test, prefer to ask the user whether
  the extra fixture cost is worth it for the change at hand.
- If you change the frontend contract, keep the frontend types aligned with the
  Rust API responses.
- `TRACK_TEST_INFERENCE=1` is an internal smoke-test seam for CLI capture.
  `TRACK_TEST_INFERENCE_RESULT` may provide the deterministic
  `ParsedTaskCandidate` JSON while the capture note itself stays realistic.
  Keep both env vars out of user-facing docs and treat them as test-only
  behavior.

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
