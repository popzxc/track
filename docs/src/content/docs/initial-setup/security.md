---
title: Security
description: Understand the safety model around autonomous remote work before you enable dispatches or delegated reviews.
sidebar:
  order: 2
---

Autonomous agents are useful precisely because they can act without waiting for permission checks. That also makes them dangerous. `track` is designed to push that risk onto a remote sandbox instead of your daily machine, but it does not remove the need for careful operating habits.

While this project aims to minimize risk for you, you still need to be cautious:

- Use a remote machine that you are okay with wiping. The `track` WebUI is resilient to that model: if the machine goes boom, you recreate it, reconnect it, and continue working. Treat the remote host as a disposable sandbox.
- Never use your main GitHub account on the remote machine. If an agent gets prompt-injected, it can leak tokens, repository data, and anything else that account can see. Create a separate GitHub account just for this workflow, and keep its standing access limited to the repositories and actions you intentionally want to expose.
- Do not blindly approve workflow runs in PRs opened by the agent. PRs from forks commonly require approval before CI starts. Inspect the diff first. Unexpected dependency version bumps, workflow changes, or unrelated infrastructure edits are all highly suspicious and can indicate prompt injection. A malicious CI workflow can leak repository secrets and gain access to sensitive automation.

If you are not comfortable with that operating model, you can still use `track` for local task capture and manual triage without dispatching autonomous work.
