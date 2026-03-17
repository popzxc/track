# track

`track` is a small personal issue tracker with a Rust backend and a Vue
frontend.

Two workflows matter:

1. capture tasks quickly from the CLI
2. review and manage tasks in a local web UI

## Quick Start

### Install the `track` CLI

From the repository root:

```bash
cargo install --path crates/track-cli --locked
```

Make sure `~/.cargo/bin` is on your `PATH`, then run:

```bash
track
```

On the first run, `track` opens an interactive config wizard. After that, you
can create tasks from anywhere with commands like:

```bash
track proj-x prio high fix a bug in module A
```

To reinstall after updating the repository:

```bash
cargo install --path crates/track-cli --locked --force
```

### Run the web UI with Docker Compose

After `track` has created `~/.config/track/config.json`, start the combined
Rust API + frontend stack with:

```bash
docker compose up --build -d
```

Then open <http://localhost:3210>.

The Compose file will build the image if needed and will automatically mount:

- `${HOME}/.track/issues` to `/data/issues`
- `${HOME}/.config/track/config.json` to `/config/config.json`

By default, the Compose service runs as `1000:1000`, which matches the common
single-user Linux setup and keeps task files writable from both Docker and the
CLI. If your host user uses a different UID or GID, start Compose with:

```bash
TRACK_UID=$(id -u) TRACK_GID=$(id -g) docker compose up --build -d
```

If you use the Docker UI together with the CLI, both use the same config file
and task directory.

To stop the stack:

```bash
docker compose down
```

If you prefer raw Docker commands instead of Compose, the previous `docker
build` / `docker run` flow still works with the same paths and environment
variables.

## Repository Shape

The repository has two top-level implementation areas:

- `crates/`
  Rust crates for the domain core, the CLI, and the HTTP API.
- `frontend/`
  The Vue/Vite frontend.

The stable boundary between them is the filesystem contract:

- `~/.config/track/config.json`
- Markdown task files under `~/.track/issues`

## Developer Setup

1. Install [Rust](https://www.rust-lang.org/tools/install) and [Bun](https://bun.sh/).
2. Run `cargo build -p track-cli`.
3. Run `cd frontend && bun install`.
4. Run `track` to generate `~/.config/track/config.json`, or create it manually.

## Example config

```json
{
  "projectRoots": [
    "/home/user/work",
    "/home/user/oss"
  ],
  "projectAliases": {
    "proj-x": "project-x-repo",
    "ethproofs": "airbender-platform"
  },
  "llamaCpp": {
    "modelPath": "/home/user/models/task-parser.gguf",
    "llamaCompletionPath": "/opt/llama.cpp/bin/llama-completion"
  }
}
```

`llamaCompletionPath` is optional. If you omit it, `track` looks for
`llama-completion` on `$PATH`.

Relative paths inside `config.json` are resolved relative to the config file
location, so the same config keeps working even when you run `track` from a
different directory.

## Common commands

- `cargo test --workspace`
- `cargo build --release -p track-cli`
- `cargo build --release -p track-api`
- `cargo run -p track-cli -- proj-x prio high fix a bug in module A`
- `cargo run -p track-api`
- `cd frontend && bun install`
- `cd frontend && bun run dev`
- `cd frontend && bun run typecheck`
- `cd frontend && bun run build`

Running `track` with no arguments opens the interactive config editor. If the
config file does not exist yet, `track ...` will launch the same setup flow
before creating the first task.

For local frontend development, `frontend/vite.config.ts` proxies `/api` and
`/health` to `http://localhost:3210` by default.

## Notes

- Task files live under `~/.track/issues` by default.
- The CLI only supports local parsing through `llama.cpp`.
- `track` uses `llama-completion` for one-shot local parsing.
- The Rust API serves both JSON routes and the built frontend assets.
- Task files keep identity in the filesystem path and keep human-editable text
  in the Markdown body; frontmatter is only for metadata.
