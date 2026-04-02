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

## Automatic review follow-ups

You can also let `track` create follow-ups automatically after PR review activity.

To enable that flow, open [Finish Runner Setup](../../configuring/finish-runner-setup/) and set both:

- **Main GitHub user**
- **Review follow-up**

Once those are configured, `track` watches for review feedback from that specific GitHub account and dispatches the next follow-up for you automatically. That means you do not have to reopen the task and manually start a new follow-up every time you leave review comments on the PR.

Automatic follow-up only listens to reviews from the configured main account.

That restriction is intentional:

- it avoids turning arbitrary reviewers into a control channel for the agent
- it keeps the automation scoped to the person who understands that review comments may be implemented directly

This matters for both safety and workflow clarity. A malicious reviewer should not be able to steer the agent, and an external reviewer may leave comments that are meant for a human to interpret rather than for an autonomous agent to apply blindly.

If somebody else leaves useful feedback, you can still read it yourself and start a manual follow-up with the instructions you actually want to send. Approved reviews are ignored.

## Tool choice stays pinned

Follow-ups do **not** switch runners midway through a thread of work. If the original run used Claude, the follow-up stays on Claude. If it used Codex, it stays on Codex.

That keeps branch history, worktree state, and prompts consistent.

## Start fresh when you really mean start fresh

If you want to throw away the old remote context and begin again:

- discard the task's run history, then dispatch again
- or create a brand-new task if the work has drifted into a different problem

Use follow-ups for continuation, not for resets.
