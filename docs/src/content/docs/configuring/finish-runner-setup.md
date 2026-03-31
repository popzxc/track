---
title: Finish Runner Setup
description: Save the WebUI settings that make remote runs and PR reviews work reliably.
sidebar:
  order: 2
---

Open the WebUI, go to **Settings**, and use **Runner setup** to finish the part that is easiest to manage in one screen.

## Shell prelude

This field is required for real remote work.

The shell prelude runs before every remote command, so it should contain the environment setup that your non-interactive SSH sessions need. A common example looks like this:

```bash
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
. "$HOME/.cargo/env"
export PATH="$PATH:/home/<your-user>/.foundry/bin"
```

Keep it focused on PATH and toolchain setup. If it prints extra text, remote command parsing becomes much less reliable.

## Preferred tool

Choose the default runner for new work:

- **Codex** if you want new task dispatches and PR reviews to default to Codex
- **Claude** if your remote machine is set up for Claude-first runs

This is only the default. You can still override it later per task dispatch or per review request.

## Review settings

If you plan to use the PR review workflow, also set:

- **Main GitHub user**: the user name the bot should refer to in review output
- **Default review prompt**: reusable guidance that gets appended to every manual review request

If you also want automatic review follow-up after relevant PR activity, turn on **Review follow-up**. Manual reviews only need the main GitHub user; automatic follow-up needs both the user and the toggle.

## Save once the form is complete

The save button stays disabled until:

- the remote agent itself has already been configured from the CLI
- the shell prelude is non-empty
- the main GitHub user is present when review follow-up is enabled
