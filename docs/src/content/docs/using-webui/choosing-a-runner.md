---
title: Choosing Codex or Claude
description: Understand where the default runner lives and when you can override it.
sidebar:
  order: 4
---

`track` supports two remote runners:

- **Codex**
- **Claude**

## Where the default lives

The default runner is set in **Settings → Runner setup** as **Preferred tool**.

That default applies to:

- new task dispatches
- new PR review requests

## Where you can override it

You can still make one-off choices:

- choose a different runner when starting a task dispatch
- choose a different runner when requesting a PR review

## What does not switch automatically

Once a task or review already has history, continuation stays on the same tool:

- task follow-ups keep the original tool
- PR re-reviews keep the original tool

If you want to switch tools after a task already ran, discard the old task run history and start fresh. If you want to switch tools for a review flow, create a new review request instead of continuing the old one.

## Practical guidance

- Use the default setting when one runner is clearly your normal choice.
- Use per-run overrides for exceptions.
- Do not switch tools midstream unless you also want a clean start.
