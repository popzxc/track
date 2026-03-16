# Architecture

`track` is a small local system built around one idea: tasks are plain files on disk, and every other part of the project exists to help create, read, and manage those files safely.

## System Shape

The project has four main parts:

- The CLI captures a free-form task description.
- The AI parser turns that free-form text into structured task data.
- The API reads and mutates the task files.
- The web UI talks to the API to display and manage tasks.

Two shared layers support those parts:

- `@track/shared` defines the common schemas and types used across the project.
- `@track/core` holds the reusable application logic: config loading, project discovery, AI provider selection, task capture rules, and filesystem-backed storage.

That split keeps the outer apps thin. The CLI, API, and web app are mostly entrypoints and adapters around the same core behavior.

## Core Runtime Flow

There are two important user flows.

### Creating a task

The CLI loads the shared config, discovers available projects from configured filesystem roots, and asks the configured AI provider to choose a project, priority, and normalized description from the user’s raw text.

If the AI result is valid and the project choice is trusted, the task is persisted as a Markdown file with YAML frontmatter under the data directory. The filesystem is the source of truth, so task creation is complete once that file exists.

### Managing tasks

The API reads tasks from the filesystem, applies sorting and update rules, and exposes a small JSON surface for the web UI. The UI does not manage its own domain state independently; it refreshes from the API so the filesystem-backed state remains canonical.

## Storage Model

Tasks live under the configured data directory, grouped by project and then by status:

- `project/open/...`
- `project/closed/...`

Closing or reopening a task is implemented as a file move between those folders plus a metadata update. Editing a task rewrites the Markdown file. Deleting a task removes the file permanently.

This design is intentionally simple: a person can inspect the data with normal shell tools or edit a task in a text editor without needing a database or migration layer.

## Provider Boundary

AI parsing is treated as a replaceable edge, not as the center of the system. The rest of the application depends on a small parser interface, and configuration decides which implementation to use.

Today there are two provider paths:

- OpenAI for hosted parsing
- `llama.cpp` via `llama-cli` for local parsing

Both providers receive the same high-level parsing contract: choose only from discovered projects, normalize the description, and fail safely when project selection is ambiguous.

## Deployment Model

The project is developed as separate apps, but deployed as one local web stack.

The backend serves both:

- `/api/*` for JSON APIs
- frontend static assets for the browser UI

That allows the Docker image to expose a single port while still keeping the frontend and backend codebases separate during development.

## How To Orient Yourself

When reading the code, it helps to think in this order:

1. `shared` defines the data contract.
2. `core` defines the behavior of the system.
3. `cli`, `api`, and `web` adapt that behavior for different entrypoints.

If you are changing business rules, start in `core`.
If you are changing data shape, start in `shared`.
If you are changing user experience, start in the app layer that owns that interaction.
