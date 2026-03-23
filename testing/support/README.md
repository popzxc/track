# Support Tooling

`fixturectl.py` is a small host-side control surface for the SSH fixture.

The script is intentionally runner-neutral:

- Rust integration tests can shell out to it.
- Future browser e2e tests can shell out to it.
- Ad-hoc local debugging can use it directly from the terminal.

The script only depends on Python stdlib plus host `docker` and `git`.

Current commands cover the setup sequence that the first live-SSH tests need:

- `build-image`
- `generate-key`
- `run`
- `wait-for-ssh`
- `seed-repo`
- `write-state`
- `stop`
