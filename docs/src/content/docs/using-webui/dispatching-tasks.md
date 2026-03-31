---
title: Dispatching Tasks
description: Capture tasks from the CLI, refine them in the WebUI, and send them to the remote runner.
sidebar:
  order: 1
---

The normal loop starts in the CLI and finishes in the WebUI.

## 1. Capture a task from the CLI

From any directory:

```bash
track todoapp fix stale task status after a canceled run
```

or 

```bash
track todoapp prio high fix stale task status after a canceled run
```

or 

```bash
track fix stale task status after a canceled run in todoapp priority low
```

You can be rough. The local parser is there to turn your note into a structured task, as long as the project name or alias resolves to something you already registered.

## 2. Review the task in the WebUI

Open the task drawer and do the light cleanup there:

- tighten the description
- adjust the priority
- close or reopen the task if needed

## 3. Dispatch the task

When the task looks good, click **Dispatch**.

The usual status sequence is:

- `Preparing`
- `Running`
- one of `Succeeded`, `Blocked`, `Failed`, or `Canceled`

If a run succeeds and opens a pull request, the task drawer shows the PR link directly.

## 4. Use the Runs page when you want history

The **Runs** page is the best place to watch active work or revisit older task runs. Use the task drawer for the current task, and the runs view when you want the bigger picture across projects.

## When dispatch is disabled

The most common missing pieces are:

- the remote agent was never configured from the CLI
- the shell prelude was never saved in the WebUI
- the project metadata is incomplete
