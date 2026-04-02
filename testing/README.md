# Testing Workspace

This directory keeps integration and e2e test infrastructure physically separate
from the product crates.

That separation is intentional:

- `crates/track-*` stays focused on application code.
- `testing/ssh-fixture/` contains the reusable remote host fixture that speaks
  real SSH while faking `gh` and `codex`.
- `testing/support/` contains host-side tooling that both Rust integration tests
  and future browser e2e tests can drive.
- `testing/e2e/` is reserved for browser-facing end-to-end tests.
- `testing/full_ci_smoke/` holds CI-only smoke flows that exercise the packaged
  installer and backend wrapper.

## Layers

We want three layers that can evolve independently:

1. External fixture contract
   This is the Docker image plus a mounted runtime directory. It should stay
   language-agnostic so Rust tests and browser runners can share it.
2. Runner-specific orchestration
   Rust integration tests can shell out to `testing/support/fixturectl.py`.
   Future Playwright or other browser tests can do the same.
3. Test suites
   API integration tests, CLI integration tests, and later frontend e2e tests
   should all reuse the same fixture instead of each inventing their own mocks.

## Current Scope

This scaffold focuses on the remote-agent boundary:

- real `ssh` and `scp`
- a real remote `git` checkout and worktree
- mocked `gh`
- mocked `codex`

That is the part of the system that is currently hardest to exercise with fast,
high-signal tests.

## Internal Test Seams

The install smoke flow uses one intentionally hidden CLI capture seam:

- `TRACK_TEST_INFERENCE=1` tells `track` capture to treat the raw capture text
  as serialized `ParsedTaskCandidate` JSON instead of running local model
  inference.
- `testing/full_ci_smoke/main.py` is CI-only on purpose. It refuses to run
  outside CI or when `~/.track` is already populated because it exercises the
  real installer and backend wrapper.

That env var exists only so the smoke suite can verify the installed CLI and
backend flow deterministically. It is not a supported user workflow and should
not appear in user-facing docs.
