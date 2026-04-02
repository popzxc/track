---
title: Intro
description: Understand what track is for, why delegation is useful, and why remote execution is part of the design.
sidebar:
  order: 1
---

`track` is for the moment when you have work in your head and you want to turn it into an actionable task quickly, without deciding the final workflow up front.

Once a task exists, you can choose what happens next:

- act on it yourself
- move it into your team issue tracker when it becomes shared work
- delegate it to an autonomous agent when you want a ready PR instead of another reminder

Delegation is useful because it offloads the task to an agent that can work without stopping for permission checks. What comes back is a PR that is ready for you to inspect, while your everyday machine stays out of the permission-bypassing execution path.

Delegated PR reviews are useful for a similar reason. You can shape the review behavior around your own preferences, and you can attach one-off instructions to a specific review request. That is much more flexible than a single shared automation that has to behave the same way for everyone.

:::caution[Delegated work requires a separate remote machine]
`track` requires a remote machine for delegated tasks and delegated PR reviews. That machine is the security boundary. Dispatches run in an autonomous mode that bypasses permission checks so the agent can finish work without interrupting you.

Treat the remote host as a resettable sandbox. If something goes wrong, the expected recovery story is to recreate the machine and continue. Do not use a machine you are not willing to wipe.
:::

Read [Security](../security/) next before you set up either machine.
