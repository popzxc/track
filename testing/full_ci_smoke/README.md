# Full CI Smoke

This directory holds the CI-only smoke flows that validate the packaged
installation path instead of the repository-local development path.

## Purpose

This smoke covers the gap that the existing integration and browser tests do
not:

- build a local backend image for the current revision
- install `track` through `trackup --ref <git-ref>`
- boot the installed backend with `track-backend`
- drive one capture, dispatch, review, and close-task flow through the installed
  CLI and live API

It also includes a deliberately harmless local scenario that only verifies the
Python venv and third-party dependency path by making a `requests` call to
`https://example.com`.

The runner intentionally refuses to run outside CI or when `~/.track` is
already populated, because it exercises the real installed wrapper scripts.

## Scenarios

- `connectivity-check`: harmless local probe that verifies the venv and
  `requests` dependency path.
- `install-flow-defaults`: CI smoke that keeps the backend port, CLI backend
  URL, remote workspace root, and remote projects registry path on their
  product defaults wherever practical.
- `install-flow-overrides`: CI smoke that passes explicit backend and
  remote-agent overrides to prove those customization paths still work.

The install scenarios still pass an explicit SSH fixture port and shell
prelude. Those are fixture-environment details rather than interesting product
defaults.

## Layout

- `main.py`: minimal CLI entrypoint
- `smoke_test/`: Python implementation package for reusable smoke machinery
- `smoke_test/scenario.py`: generic scenario and action runner
- `smoke_test/scenarios.py`: declarative smoke definitions
- `smoke_test/actions.py`: high-level smoke steps
- `smoke_test/smoke_context.py`: runtime state and temp-path allocation
- `smoke_test/shell_utils.py` and `smoke_test/api_client.py`: external process
  and HTTP helpers
- `requirements.txt`: runtime dependencies for the venv-based CI execution

## Entrypoint

The intended execution path is:

```bash
python3 -m venv testing/full_ci_smoke/.venv
testing/full_ci_smoke/.venv/bin/pip install -r testing/full_ci_smoke/requirements.txt
testing/full_ci_smoke/.venv/bin/python testing/full_ci_smoke/main.py --scenario connectivity-check
testing/full_ci_smoke/.venv/bin/python testing/full_ci_smoke/main.py --scenario install-flow-defaults --revision <git-ref>
testing/full_ci_smoke/.venv/bin/python testing/full_ci_smoke/main.py --scenario install-flow-overrides --revision <git-ref>
```
