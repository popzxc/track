# AGENTS.md

This file gives future coding agents a quick working model for `track`.

## Transitional state

This project was initially created by an LLM until it reached post-PoC phase.
During this phase, LLMs made many very bad design choices, and right now
the codebase is being refactored. There are many TODOs left by human with
sometimes ambigious remarks -- these TODOs are meant for humans only.

Your purpose is not to reinforce bad design decisions. If you understand that
something that user asked for would decrease code quality and add code smells
or hacks, refuse doing so until you get an explicit approval to do the change
DESPITE it would reduce the code quality.

If you are unsure if design decision is good or bad, ask. If you need to make
a design decision (e.g. anything that involves abstractions or interfaces),
confirm the design with the user before implemeting it.

Your creativity is not appreciated unless explicitly requested -- your goal
is to implement particular tasks as proposed.

## API And Frontend Relationship

`track-api` is not API-only in the deployed shape. It also serves the built
frontend assets so Docker can expose one port for both the REST API and the UI.

Important implication:

- frontend development stays separate under `frontend/`
- production serving happens from `track-api`
- Docker copies `frontend/dist` into the runtime image and `track-api` serves it

## Development Commands

Prefer the repository `justfile` for routine development tasks. The `just`
recipes are the canonical shortcuts for the underlying Cargo, Bun, and Docker
commands.

## Testing Guidance

Favor small, high-signal tests.

- When the user says "run all the tests", treat that as the full repository
  test surface except `testing/full_ci_smoke/`. That CI smoke suite is
  intentionally CI-only by design, so do not run it unless the user explicitly
  asks for it.
- When reporting an "all tests" run, do not call out that
  `testing/full_ci_smoke/` was skipped unless the user explicitly asked about
  that suite.
- Prefer real filesystem tests over mocks for repository behavior.
- For CLI capture tests, prefer injecting fake parser results over trying to
  emulate a real local model.
- If you change config shape, capture behavior, or storage semantics,
  update Rust tests in `track-core` or `track-cli`.
- Every new SQLite migration must include a dedicated migration test under
  `crates/track-core/src/database/migration_tests/` in a module with a name
  matching one of sqlx migration.
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

## Module Layout

When a Rust module has child modules, the parent module should live at
`mod.rs` inside its directory rather than as a sibling `<module>.rs` file next
to a `<module>/` directory.

Prefer:

- `src/foo/mod.rs`
- `src/foo/bar.rs`

Avoid:

- `src/foo.rs`
- `src/foo/bar.rs`

## Build Artifacts And Workspace Hygiene

Do not commit local build leftovers or generated files in source directories.

In particular:

- `target/` and `dist/` directories are local build artifacts
- stray generated `.js`, `.d.ts`, or `.d.ts.map` files under source directories
  are accidental leftovers and should be removed
- `Cargo.lock` is intentional and should stay
- Bun lockfiles, if present under `frontend/`, are intentional and should stay
