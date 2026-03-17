# Architecture

`track` is a small local system built around one stable idea: tasks are plain
files on disk, and every part of the project exists to either create those
files or help a person manage them safely.

## System Shape

The project now has one Rust backend and one TypeScript frontend.

- `crates/track-core`
  Shared Rust domain logic: config loading, project discovery, `llama-completion`
  parsing, task storage, and sorting.
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
5. writes a Markdown task file to disk

The local model is advisory, not authoritative. It may infer from discovered
projects, but it is not allowed to invent arbitrary destinations.

## Management Flow

Task management lives in the Rust API plus the frontend.

The API reads task files from disk, applies sorting and mutation rules, and
exposes a small JSON surface. The frontend talks to that API and treats the
filesystem-backed state as canonical.

This keeps the frontend thin and lets the CLI and UI converge on the same task
model without duplicating backend rules.

## Storage Model

Tasks live under the configured data directory, grouped by project and then by
status:

- `project/open/...`
- `project/closed/...`

Closing or reopening a task is a file move plus metadata update. Editing a task
rewrites the Markdown file. Deleting a task removes the file.

Task identity lives in the filesystem path. The Markdown body is the
human-editable description. Frontmatter is reserved for metadata such as
priority and timestamps.

## Deployment Model

The deployed web stack is still a single local server process.

`track-api` serves both:

- `/api/*` for JSON endpoints
- built frontend assets for the browser UI

That keeps Docker simple: one image, one process, one port.
