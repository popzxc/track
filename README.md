# track

`track` is a small issue tracker with three moving parts:

- a local CLI that captures tasks from rough notes
- a local WebUI for editing, dispatching, and reviewing
- a remote runner that can use Codex or Claude

## Documentation

The canonical documentation is published at [https://popzxc.github.io/track/](https://popzxc.github.io/track/), and its source lives under [`docs/`](./docs/).

Start here:

- [Initial setup](https://popzxc.github.io/track/initial-setup/local-and-remote-prerequisites/)
- [Configuring projects and runner settings](https://popzxc.github.io/track/configuring/register-projects/)
- [Using the WebUI](https://popzxc.github.io/track/using-webui/dispatching-tasks/)
- [Reference](https://popzxc.github.io/track/reference/config-files/)
- [Development flow](https://popzxc.github.io/track/development-flow/development-flow/)

## Local docs development

```bash
cd docs
bun install
bun run dev
```

## Repository shape

- `crates/track-core`: shared backend behavior and remote-agent orchestration
- `crates/track-capture`: local parsing and model resolution
- `crates/track-cli`: CLI entrypoint
- `crates/track-api`: Axum backend and static asset serving
- `frontend/`: Vue WebUI
- `docs/`: Astro Starlight documentation

## License

`track` is licensed under [MIT](./LICENSE).
