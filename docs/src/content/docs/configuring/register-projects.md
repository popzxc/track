---
title: Register Projects
description: Teach track which repositories exist before you start capturing tasks into them.
sidebar:
  order: 1
---

`track` only captures tasks into projects that the backend already knows about. The smoothest setup is to register each repository once, then use either its canonical name or one of its aliases in day-to-day task capture.

## Register from a checkout

From inside a local Git checkout:

```bash
track project register
```

To add one or more aliases at the same time:

```bash
track project register --alias todoapp --alias todo
```

You can also register an explicit path instead of using the current directory:

```bash
track project register ~/workspace/project-a --alias payments
```

## What registration does

When you register a project, `track`:

- uses the checkout directory name as the canonical project name
- stores any aliases you provided
- tries to infer repo metadata such as the repository URL and base branch

That inferred metadata is usually enough to get started, but you should still verify it in the WebUI before your first dispatch.

## After registration

Once a project is registered, you can capture tasks against it from anywhere. For example:

```bash
track todoapp redo the todo view
```

This usually routes the task to `todoapp` with the default medium priority.
If you provide more context, the local parser can infer more details too. For example:

```bash
track todoapp prio high tighten retry logic around remote cleanup
```

That task would be created with a high priority.

Any created task can be edited in the WebUI after creation, so in CLI you can draft a quick idea just to not forget, and then
refine it later in the WebUI when you have time.

Tip: use ticks to avoid escaping, e.g.:

```bash
track todoapp 'some text with `backticks` that will confuse your shell otherwise'
```

If you skip project registration entirely, task capture will fail because the backend has no valid destination projects to offer the local parser.
