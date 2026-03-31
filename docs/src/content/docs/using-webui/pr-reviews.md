---
title: PR Reviews
description: Ask the remote runner to review a pull request directly on GitHub and keep a history of those runs.
sidebar:
  order: 3
---

The review flow is separate from task dispatches, but it follows the same basic idea: the local WebUI orchestrates the work and the remote runner does the GitHub-facing part.

## Before requesting a review

Make sure you already saved:

- a shell prelude in **Settings**
- a **Main GitHub user** in **Runner setup**

Without those, the review flow stays disabled.

## Request a review

In the **Reviews** page:

1. click **Request a review**
2. paste a full GitHub pull request URL
3. optionally add one-off instructions
4. choose Codex or Claude for this review

The agent will post the review directly on GitHub and the WebUI will keep the local run history.

## Re-reviews

When the pull request changes, ask for a re-review instead of creating a separate review thread. `track` keeps the previous review context and records each run so you can see how the review evolved.

## Default review prompt

If you saved a default review prompt in **Settings**, it is appended to every manual review request before the one-off instructions you type for that specific run.

You can think of it as an `AGENTS.md` for reviews specifically: one shared prompt that applies to every review request unless you override it with extra instructions for that run.

That is a good place for standing guidance such as:

- prioritize bugs and regressions
- call out risky behavior changes
- prefer high-signal findings over style commentary
