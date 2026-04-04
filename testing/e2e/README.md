# Browser E2E Space

This directory holds the sparse browser end-to-end tests for `track`.

Those tests intentionally reuse the same external fixture contract as the Rust
integration tests:

- start the SSH fixture with `testing/support/fixturectl.py`
- build the real frontend
- start a real `track-api` process against temporary state
- drive the browser against that API-served frontend

That keeps browser e2e focused on true full-stack behavior instead of inventing
its own mock-only environment.

## Scope

These tests are intentionally expensive and few. They should cover only the
highest-signal workflows, such as:

- dispatching a task from the UI and observing the resulting PR state
- continuing a task with a follow-up request

They are not intended to replace the fast frontend unit/component suite.

## Running

Before running, you need to install chromium:

```sh
cd frontend
bunx playwright install chromium
```

Or (CI variant):

```sh
cd frontend
bunx playwright install --with-deps chromium
```

From `frontend/`:

- `bun run test` for the fast unit/component suite
- `bun run test:e2e` for the browser end-to-end suite
