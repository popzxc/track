# Testing Workspace

This directory keeps integration and e2e test infrastructure physically separate
from the product crates.

That separation is intentional:

- `crates/track-*` stays focused on application code.
- `testing/ssh-fixture/` contains the reusable remote host fixture that speaks
  real SSH while faking `gh` and `codex`.
- `testing/support/` contains host-side tooling that both Rust integration tests
  and future browser e2e tests can drive.
- `testing/e2e/` is reserved for full-stack scenarios that may later include the
  frontend.

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
