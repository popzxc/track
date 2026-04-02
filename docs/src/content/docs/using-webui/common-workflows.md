---
title: Common Workflows
description: Practical advice for configuring prompts, handling PRs, and getting better signal from remote runs.
sidebar:
  order: 5
---

This page is a set of practical suggestions for people who already finished setup and want a smoother day-to-day workflow with `track`.

For the mechanics, see [Dispatching Tasks](../dispatching-tasks/), [Follow-Ups](../follow-ups/), and [PR Reviews](../pr-reviews/). This page is about how to use those flows deliberately.

## Layer your instructions on purpose

`track` launches Codex or Claude for you, but it does not manage the runner's native configuration files. If you installed those tools on the remote VM yourself, `AGENTS.md`, `CLAUDE.md`, and similar tool-native files are still yours to shape.

Use each layer for a different job:

- use `AGENTS.md` or `CLAUDE.md` for durable defaults such as coding style, output style, commit conventions, or how much detail you want in summaries
- use reusable `track` prompts for standing guidance you want on a class of runs
- use the task description, follow-up request, or review request for the context that matters right now

For reviews, the reusable `track` layer today is **Default review prompt** in **Settings -> Runner setup**. That is a good place to describe how you want reviews from your bot to look in general, for example whether it should emphasize bugs, testing gaps, caveats, or implementation tradeoffs.

The task body, manual follow-up request, and review request are the best place for situation-specific context: what changed, where to focus, what the bot should double-check, or what hacks and caveats should be called out explicitly.

If you care about model choice or reasoning effort, configure that in the remote Codex or Claude setup itself. `track` does not choose those settings for you, so leaving the CLI defaults in place can send runs through the wrong model or the wrong reasoning level.

## Pick manual or automatic PR follow-up deliberately

Once a bot-created PR exists, you usually have two ways to continue:

- leave a normal GitHub review and let automatic follow-up handle the next round
- start a manual follow-up from the WebUI

Use automatic follow-up for ordinary review rounds when you want your own comments applied with minimal friction. Remember that automatic follow-up only listens to reviews from the configured **Main GitHub user**.

Use a manual follow-up when:

- the bot missed the original request and you want tighter steering
- you do not want the instruction to be visible in a public review
- you want the bot to react to feedback from somebody else

In the manual case, paste the relevant review link or summarize the exact feedback you want the bot to address.

## Treat PR reviews as remote QA runs

A review request does not have to mean "give me a generic AI review."

Because the runner checks out the repository on the remote machine, you can use review instructions for extra work such as:

- running specific checks or scripts
- doing manual QA
- using external tools that are available on the runner
- checking behavior across multiple repositories
- focusing on the risky parts of the diff instead of the whole PR equally

That usually produces much better review signal than a generic review request with no focus.

## Use one runner to check another

When `track` opens a PR, it can still be useful to request a review from `track` on that same PR.

Common patterns include:

- create with Codex, review with Claude
- create with Claude, review with Codex
- create and review with the same runner when you mainly want a second pass before merging

After the review lands, start a manual follow-up and point the bot at the review you want fixed. This is a practical way to turn review findings into another autonomous implementation pass.

## Debug CI carefully

`track` works through forks, and PRs from forks often do not start CI automatically.

If your repository does not use secrets in CI, one useful workflow is to ask the bot to open a PR in its fork first, wait for CI there, and only then open or update the main PR once the checks look clean.

Even in that flow, inspect the diff yourself before approving CI on the main repository. Fork-based automation is convenient, but it is still autonomous code and should be treated accordingly.
