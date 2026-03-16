# track

`track` is a small personal issue tracker built around two workflows:

1. Capture tasks quickly from the CLI.
2. Review and manage tasks in a local web UI.

The repository is organized as a Bun workspace with:

- `apps/cli` for the `track` command.
- `apps/api` for the Hono backend and static file serving.
- `apps/web` for the Vue frontend.
- `packages/shared` for shared schemas and types.
- `packages/core` for reusable filesystem, config, project, and AI services.

## Setup

1. Install [Bun](https://bun.sh/).
2. Run `bun install` in the repository root.
3. Create `~/.config/track/config.json`.
4. Export `OPENAI_API_KEY`.

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
  "ai": {
    "provider": "openai",
    "openai": {
      "model": "gpt-4.1-nano"
    }
  }
}
```

To use a local `llama.cpp` model instead:

```json
{
  "projectRoots": [
    "/home/user/work"
  ],
  "projectAliases": {},
  "ai": {
    "provider": "llama-cpp",
    "llamaCpp": {
      "modelPath": "/home/user/models/task-parser.gguf",
      "llamaCliPath": "/opt/llama.cpp/bin/llama-cli"
    }
  }
}
```

## Common commands

- `bun run build`
- `bun run test`
- `bun run dev:api`
- `bun run dev:web`
- `bun run dev:cli -- proj-x prio high fix a bug in module A`

## Docker

```bash
docker build -t track-web .
docker run -d \
  -p 3210:3210 \
  -v ~/.track/issues:/data/issues \
  -v ~/.config/track/config.json:/config/config.json:ro \
  -e TRACK_DATA_DIR=/data/issues \
  -e TRACK_CONFIG_PATH=/config/config.json \
  -e OPENAI_API_KEY=your-key \
  track-web
```

## Notes

- Task files live under `~/.track/issues` by default.
- The OpenAI-backed parser is isolated behind an interface so we can swap providers later without rewriting the CLI workflow.
- `ai.provider` can be set to `openai` or `llama-cpp`; when `llama-cpp` is selected, `ai.llamaCpp.modelPath` is required.
- `ai.llamaCpp.llamaCliPath` is optional and lets you point at an absolute `llama-cli` binary that is not on `$PATH`.
