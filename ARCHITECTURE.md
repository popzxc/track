# Architecture

`track` is a small local system built around one stable idea: tasks are plain
files on disk, and every part of the project exists to either create those
files or help a person manage them safely.

## System Shape

The project now has one Rust backend and one TypeScript frontend.

- `crates/track-core`
  Shared Rust domain logic: config loading, CLI-side project discovery,
  `llama-completion` parsing, project metadata persistence, task storage, and
  sorting.
- `crates/track-cli`
  The CLI adapter over `track-core`.
- `crates/track-api`
  The Axum server over `track-core`.
- `frontend/`
  The Vue/Vite UI.

The backend and frontend do not share implementation code directly. They share
the on-disk data model and the HTTP contract.

## Capture Flow

Task creation lives in the Rust CLI.

The CLI:

1. loads config
2. discovers git repositories under configured roots
3. asks `llama-completion` to normalize the raw text
4. validates that the chosen project really exists
5. initializes `PROJECT.md` for the chosen repository when needed, including an inferred default branch
6. writes a Markdown task file to disk
7. sends a best-effort local API notification so the browser UI can refresh

The local model is advisory, not authoritative. It may infer from discovered
projects, but it is not allowed to invent arbitrary destinations.

## Management Flow

Task management lives in the Rust API plus the frontend.

The API reads task files and project directories from disk, applies sorting and
mutation rules, and exposes a small JSON surface. The frontend talks to that
API and treats the filesystem-backed state as canonical. It also polls a small
task-change version endpoint so CLI-created tasks appear automatically when the
local API is running.

This keeps the frontend thin and lets the CLI and UI converge on the same task
model without duplicating backend rules.

## Remote Dispatch Flow

Remote execution is intentionally split between the local API and the remote
machine.

The API owns orchestration because the browser should not need SSH access or
direct knowledge of remote workspace layout. When a person clicks `Dispatch`,
the API:

1. loads the task and project metadata from the local track directory
2. connects to the remote machine with a managed SSH key
3. runs the configured `remoteAgent.shellPrelude` so non-interactive SSH
   commands get the same PATH/toolchain setup the remote runner needs
4. ensures the remote checkout and worktree exist
5. uploads a prompt file plus a JSON output schema
6. launches `codex exec` in fully autonomous mode
7. persists a local dispatch record so the frontend can poll status

The remote machine owns the mutable developer workspace. It keeps a small JSON
registry of prepared project checkouts so later dispatches can reuse the same
clone instead of re-bootstrapping everything each time.

## Storage Model

Tasks live under the configured data directory, grouped by project and then by
status:

- `project/open/...`
- `project/closed/...`

Each project directory can also contain `project/PROJECT.md`, which stores repo
URL, git URL, inferred-or-overridden base branch, and optional description for
the frontend editor.

The data directory also contains two hidden implementation areas:

- `.dispatches/` for local dispatch status records
- `../remote-agent/` for the managed SSH key and `known_hosts`

Closing or reopening a task is a file move plus metadata update. Editing a task
rewrites the Markdown file. Deleting a task removes the file.

Task identity lives in the filesystem path. The Markdown body is the
human-editable description. Newer CLI-captured tasks separate `## Summary` from
`## Original note` so remote automation can distinguish model-shaped prose from
the raw user request. Frontmatter is reserved for metadata such as priority and
timestamps.

## Deployment Model

The deployed web stack is still a single local server process.

`track-api` serves both:

- `/api/*` for JSON endpoints
- built frontend assets for the browser UI

That keeps Docker simple: one image, one process, one port.
