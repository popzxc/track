# Setup

This guide covers:

- local task capture through in-process `llama.cpp` bindings
- a local web UI running in Docker
- a remote machine that can run `codex` autonomously and open PRs

Assumptions:

- your local machine is where you run `track` and open the web UI
- your remote machine is a Linux VM
- GitHub is the forge you want the remote agent to use

Phases:

1. rent and configure the remote VM
2. prepare everything for the local `track` app
3. prepare everything for the web UI and first dispatch

## 1. Rent & configure the remote VM

### 1.1 Create a dedicated GitHub account for agentic work

Create a separate GitHub user for automation. Do not reuse your personal
account if you want the remote agent's forks, PRs, and tokens to stay
isolated.

Then create a token for that account with the repository permissions you need.

At minimum, the remote agent flow should be able to:

- authenticate with `gh`
- fork repositories
- push branches
- open PRs

### 1.2 Rent and harden a Linux VM

Any Linux VM is fine. A small Hetzner instance is a reasonable default.

Before installing tooling, do a standard server hardening pass:

- create a non-root user
- disable direct root SSH access if that matches your security posture
- set up `ufw`
- set up `fail2ban`

You do not need Docker on the remote machine for `track`. Install it there
only if you want it for unrelated reasons, but if so, make sure to harden
it too -- it requires additional setup to work well with `ufw`.

If the above sounds hard, you can ask ChatGPT or any other model of your choice
to generate a setup script for you -- LLMs are pretty good at it. Just make sure
that after running you verify that everything works (you can use LLM for that too).

### 1.3 Create a dedicated SSH key for `track`

Do not reuse an existing key. `track` copies this private key into its managed
automation directory, so it should be dedicated to this flow.

This key is only for your local machine to reach the VM. It is separate from
the GitHub SSH key that the VM will use later.

Create the key on your local machine:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/track_remote_agent -C "track remote agent"
```

Install the public key on the remote VM:

```bash
ssh-copy-id -i ~/.ssh/track_remote_agent.pub <remote-user>@<remote-ip>
```

If you prefer, you can add the public key to the remote user's
`~/.ssh/authorized_keys` manually instead.

Verify local-to-remote SSH works:

```bash
ssh -i ~/.ssh/track_remote_agent <remote-user>@<remote-ip>
```

### 1.4 Install development tools on the remote VM

Install the basics first:

```bash
sudo apt update
sudo apt install -y git curl ca-certificates build-essential
```

You also need:

- Node.js and `npm` (you can use `nvm` for that)
- `gh`
- `codex`

### 1.5 Configure GitHub access on the remote VM

First, install GitHub CLI using the official method for your distro, then
authenticate it with the token you created for the dedicated GitHub automation
account.

One common flow is:

```bash
gh auth login --with-token
```

Then paste the token on stdin.

Verify it worked:

```bash
gh auth status
```

Next, make sure the remote VM can use GitHub SSH URLs. `track` currently
clones and pushes using `git@github.com`, so the remote machine must be able
to talk to GitHub over SSH.

Normally, when you initialize `gh`, it would suggest creating a dedicated SSH
token for you, assuming that your token has this permission. However, you can
generate SSH key yourself and add it to the user through GitHub UI.

### 1.6 Install Codex on the remote VM

Install and set up [Codex CLI](https://openai.com/codex/).

You can also create your own `AGENTS.md` with your preferences -- `track` will give
the instructions on how to do tasks together with the task itself, so you don't need
to explain that, but you might want to customize how it writes code, does commits, etc.

### 1.7 Capture the shell prelude for non-interactive runs

`track` launches the remote runner through a non-interactive shell. Tooling
that appears in your interactive shell can still fail at runtime if that shell
does not export the same environment non-interactively.

Collect your environment setup into a shell snippet that you will paste into
the web UI later.

Typically, you need to check your `.bashrc` and copy relevant parts.

For example:

```bash
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
[ -s "$NVM_DIR/bash_completion" ] && . "$NVM_DIR/bash_completion"
. "$HOME/.cargo/env"
export PATH="$PATH:/home/<your_user>/.foundry/bin"
```

Keep this snippet quiet. It should prepare `PATH` and environment variables,
but it should not print banners or prompts.

You will need it later, when we'll be setting up the web UI.

## 2. Prepare everything for the local `track` app

### 2.1 Install Rust locally

You need Rust locally to build and install `track`.

Install with:

```bash
curl https://sh.rustup.rs -sSf | sh
```

Then open a new shell or load Cargo into your current one:

```bash
. "$HOME/.cargo/env"
```

### 2.2 Install local build prerequisites

`track-cli` builds the local `llama.cpp` backend through Rust bindings, so your
machine needs the usual native build tooling plus `libclang`.

On Debian or Ubuntu, a good baseline is:

```bash
sudo apt update
sudo apt install -y build-essential cmake clang libclang-dev pkg-config
```

If you use another distro or macOS, install the equivalent C/C++ toolchain,
CMake, and `libclang` package for your platform.

### 2.3 Choose a local model source

`track` uses a local GGUF model for task capture.

By default, it downloads this model into `~/.track/models` on first use:
[Meta-Llama-3-8B-Instruct-Q4_K_M.gguf](https://huggingface.co/bartowski/Meta-Llama-3-8B-Instruct-GGUF?show_file_info=Meta-Llama-3-8B-Instruct-Q4_K_M.gguf)

If you want to use a different model, `track` supports two manual override
shapes in `~/.config/track/config.json`:

1. Set `llamaCpp.modelPath` to a local GGUF file you manage yourself.
2. Set both `llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile` to a different
   Hugging Face model file.

If you prefer a manual local file, create a directory for models:

```bash
mkdir -p ~/.models
```

Then download a quantized instruction-tuned model of your choice and put it
somewhere stable, for example:

```text
~/.models/Meta-Llama-3-8B-Instruct-Q4_K_M.gguf
```

Write down the final absolute path. You will need it only if you plan to set
`llamaCpp.modelPath` manually.

### 2.4 Clone this repository locally

```bash
git clone <your-track-repo-url>
cd track
```

### 2.5 Install the `track` CLI locally

From the repository root:

```bash
cargo install --path crates/track-cli --locked
```

Make sure `~/.cargo/bin` is on your `PATH`.

Verify:

```bash
track --help
```

### 2.6 Run `track` and complete the first-run wizard

Run:

```bash
track
```

The wizard will create `~/.config/track/config.json`.

Here is what it will ask for and what you should provide:

- `API port`
  `3210` is the default and is usually the right choice.
- `Project roots`
  Add the local directories that contain the Git repositories you want `track`
  to discover.
- `Project aliases`
  Optional. Use these if you want short names like `airbender` to map to a
  canonical repo name like `zksync-airbender`.
- `Remote agent host`
  The public IP or hostname of your VM.
- `Remote agent user`
  Your non-root user on the VM.
- `Remote agent port`
  Usually `22`.
- `Remote workspace root`
  `~/workspace` is the default and is a good choice.
- `Remote projects registry path`
  `~/track-projects.json` is the default and is a good choice.
- `SSH private key to import`
  Point this at the dedicated key you created earlier, for example
  `~/.ssh/track_remote_agent`.

After the wizard finishes, the first task capture will download the default
local model into `~/.track/models` if it is not already cached.

If you want to override the model later, edit `~/.config/track/config.json`
directly and set either `llamaCpp.modelPath` or both
`llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile`.

When you import the key, `track` copies it into its managed automation
directory under `~/.track/remote-agent/`. That is why the key must be dedicated
to this workflow.

## 3. Prepare everything for the web UI

### 3.1 Make sure Docker is available locally

The web UI is started with Docker Compose on your local machine, so make sure
both `docker` and `docker compose` are available before continuing.

Check:

```bash
docker compose version
```

### 3.2 Start the local web UI with Docker Compose

From the repository root:

```bash
docker compose up --build -d
```

Then open:

```text
http://localhost:3210
```

If your local user is not `1000:1000`, start it like this instead:

```bash
TRACK_UID=$(id -u) TRACK_GID=$(id -g) docker compose up --build -d
```

### 3.3 Open `Runner setup` in the web UI

Paste the shell snippet from section 1.7 here so the remote runner can
reconstruct your toolchain environment in a non-interactive shell.

If you are unsure what to paste, check your remote `~/.bashrc` or `~/.zshrc`
and copy only the parts that set up paths and tooling.

### 3.4 Create your first task

From any directory on your local machine:

```bash
track airbender prio high fix README.md link to /latest
```

The project text should match either:

- a repository discovered under one of your configured project roots
- or a configured alias

This creates a Markdown task file under `~/.track/issues/...`.

### 3.5 Verify project details in the web UI

Open the UI, select the task's project, and click `Project details`.

For remote dispatch to work, the project must have:

- repo URL
- git URL
- base branch

If the task came from a repository that `track` could inspect locally, these
may already be filled in for you. Verify them anyway before your first
dispatch.

### 3.6 Dispatch the agent

Open the task card and click `Dispatch`.

The normal lifecycle is:

- `Preparing environment`
- `Agent running`
- `Succeeded`, `Blocked`, or `Failed`

Successful runs should surface the PR link directly in the task card.

Dispatch state is persisted, so if you restart your local machine and launch
the Docker stack again later, the UI can still recover and show the latest
known outcome.

## Common first-run problems

### `codex: not found`

Your remote runner environment is incomplete. Update `Runner setup` in the web
UI so the non-interactive shell can find `codex`.

### GitHub clone or push fails

The remote VM likely is not authenticated correctly for `git@github.com`.
Re-check:

```bash
gh auth status
ssh -T git@github.com
```

### `track` cannot import the SSH key

Check permissions on both:

- the original source key, for example `~/.ssh/track_remote_agent`
- `~/.track/remote-agent/`

### Dispatch button is disabled

Usually one of these is missing:

- remote agent config in `track`
- `Runner setup` shell snippet
- project metadata in `Project details`

## What you end up with

After setup, the important local files are:

- `~/.config/track/config.json`
- `~/.track/issues/`
- `~/.track/remote-agent/`

And the important remote pieces are:

- a working `codex` installation
- a working `gh` session
- GitHub SSH access
- a workspace root such as `~/workspace`
- a project registry such as `~/track-projects.json`

Normal loop:

1. create a task with `track ...`
2. refine it in the web UI if needed
3. click `Dispatch`
4. review the resulting PR
