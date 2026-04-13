# Architecture

The canonical architecture write-up now lives in the Starlight documentation book:

- [System structure](https://popzxc.github.io/track/development-flow/system-structure/)
- [Development flow](https://popzxc.github.io/track/development-flow/development-flow/)

Short version:

- `track-cli` captures tasks with a local parser and talks to the backend over HTTP.
- `track-api` owns registered projects, tasks, runs, reviews, migration state, and remote-agent settings.
- `track-core` contains the shared repositories and orchestration logic.
- `frontend/` is the local WebUI served by the backend.
- `docs/` is the Starlight book for user and contributor documentation.

Current live state is backend-centered:

- CLI config lives in `~/.config/track/cli.json`
- backend state lives in SQLite
- managed remote-agent SSH material lives under backend state
- older `config.json`-based layouts are migration inputs, not the primary live contract

`just` is used for task management in the repository.