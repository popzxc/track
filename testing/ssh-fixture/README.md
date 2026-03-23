# SSH Fixture

This Docker image acts as a small remote machine for `track` integration tests.

It intentionally keeps one part real and two parts fake:

- real `sshd`
- real `scp`
- real `git`
- mocked `gh`
- mocked `codex`

That split gives us coverage for the shelling-out and remote filesystem flows
that matter in production without depending on authenticated GitHub or Codex.

## Runtime Contract

The fixture mounts one host directory at `/srv/track-testing`.

Inside that runtime directory:

- `authorized_keys`
  Public keys that should be accepted by `sshd` for the `track` user.
- `state/gh.json`
  Declarative state that drives the `gh` mock.
- `state/codex.json`
  Declarative state that drives the `codex` mock.
- `logs/`
  JSONL invocation logs emitted by the mocks.
- `git/`
  Bare upstream and fork repositories visible to the mock `gh` implementation.

The mocks log every invocation so tests can assert both outcomes and intent.

## Shell Prelude

`track` currently requires a non-empty remote shell prelude before dispatching.
Tests should therefore configure the remote agent with something like:

```sh
export PATH="/opt/track-testing/bin:$PATH"
export TRACK_TESTING_RUNTIME_DIR="/srv/track-testing"
```

That keeps the mocks explicit and also exercises the shell-prelude path that
production dispatches rely on.

## Why Python Mocks

The wrappers are shell scripts so they look like normal CLI binaries on `PATH`,
but the mock logic lives in Python.

That gives us:

- predictable JSON parsing and serialization
- room to add more subcommands later without shell-script sprawl
- simple subprocess and filesystem handling for real git side effects
