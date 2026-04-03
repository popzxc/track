---
title: Remote Prerequisites
description: Prepare the remote machine that will run autonomous tasks and delegated PR reviews.
sidebar:
  order: 4
---

This page is about the remote machine that will run delegated tasks and delegated PR reviews. It should be a Linux VM or similar host that you explicitly treat as disposable.

## 1. Create a dedicated GitHub account for agentic work

You are expected to create a dedicated GitHub account just for the agentic work. Do not use your personal account. See [Security](../security/) for details.

Keep that account's standing access limited to the repositories and actions you intentionally want to expose. At minimum, the remote flow should be able to:

- authenticate with `gh`
- clone or fork repositories
- push branches
- open pull requests

## 2. Provision a resettable Linux VM

Any Linux VM is fine as long as you can recreate it without drama.

Before installing developer tooling, do a normal hardening pass:

- create a non-root user
- disable direct root SSH access if that fits your security posture
- set up `ufw`
- set up `fail2ban`

You do not need Docker on the remote machine for `track`. Install it there only if you need it for unrelated reasons.

The important property is not "pet server" reliability. The important property is that this machine is a resettable sandbox. If it gets compromised or confused, you wipe it, recreate it, and continue.

## 3. Install the remote toolchain

Install the basics first:

```bash
sudo apt update
sudo apt install -y git curl ca-certificates build-essential
```

Then install the rest of the toolchain you want the remote runner to use:

- Node.js and `npm` if your runner install flow expects them
- `gh`
- `codex`, `claude`, or both, depending on which runner(s) you plan to use

After installation, verify the commands exist in a normal shell on the remote machine.

## 4. Configure GitHub access on the remote machine

`track` clones and pushes with `git@github.com`, so the remote machine must be able to talk to GitHub both through `gh` and through SSH.

First, authenticate GitHub CLI as the dedicated automation user. A common flow is:

```bash
gh auth login --with-token
```

Then paste the token on stdin and verify the session:

```bash
gh auth status
```

Next, set up a GitHub SSH key on the remote machine for that dedicated GitHub account. This key belongs on the remote host and is separate from the local-to-remote SSH key you create later for `track`.

Verify GitHub SSH access:

```bash
ssh -T git@github.com
```

If either check shows your personal GitHub account, stop and fix that before you continue.

## 5. Create a dedicated SSH key from your local machine to the remote machine

Create a dedicated SSH key for `track` on your local machine. This must be a dedicated key because `track` copies it into its managed automation directory and uses it for remote dispatches. Do not reuse SSH keys that are already used for GitHub, for other servers, or for any unrelated purpose.

This key is only for your local machine to reach the remote host. It is separate from the GitHub SSH key that the remote machine uses to talk to GitHub.

Create the key locally:

```bash
ssh-keygen -t ed25519 -f ~/.ssh/track_remote_agent -C "track remote agent"
```

When prompted for a passphrase, leave it empty. `track` uses this key for automated remote dispatches and cannot supply a passphrase interactively.

Install the public key on the remote machine and verify login works:

```bash
ssh-copy-id -i ~/.ssh/track_remote_agent.pub <remote-user>@<remote-host>
ssh -i ~/.ssh/track_remote_agent <remote-user>@<remote-host>
```

If you prefer, you can add the public key to `~/.ssh/authorized_keys` manually instead.

## 6. Collect a quiet shell prelude for non-interactive runs

Later, the WebUI will ask for a shell prelude. That snippet runs before every remote command, so it needs to reconstruct the environment that your non-interactive SSH sessions require.

Most likely, you'll get something that looks like this:

```bash
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
[ -s "$NVM_DIR/bash_completion" ] && . "$NVM_DIR/bash_completion"
. "$HOME/.cargo/env"
export PATH="$PATH:/home/<your-user>/.local/bin"
```

Keep it quiet on stdout. It should set environment variables and `PATH` entries, but it should not print banners, prompts, or status messages.

When you later run `track remote-agent configure`, the defaults `~/workspace` and `~/track-projects.json` are reasonable choices for most setups.
