---
title: Follow-Ups
description: Continue work on the same branch, worktree, or pull request instead of starting over.
sidebar:
  order: 2
---

Use a follow-up when you want the agent to continue existing work rather than start from a clean slate.

Common examples:

- `Address the PR review comments.`
- `Rework this to avoid cloning the config.`
- `Add regression tests for the failing edge case.`

## What a follow-up reuses

If the latest task run already has a pull request, the follow-up continues on that same PR and branch.

If there is no PR yet, the follow-up still reuses the existing remote branch and worktree.

## Tool choice stays pinned

Follow-ups do **not** switch runners midway through a thread of work. If the original run used Claude, the follow-up stays on Claude. If it used Codex, it stays on Codex.

That keeps branch history, worktree state, and prompts consistent.

## Start fresh when you really mean start fresh

If you want to throw away the old remote context and begin again:

- discard the task's run history, then dispatch again
- or create a brand-new task if the work has drifted into a different problem

Use follow-ups for continuation, not for resets.
