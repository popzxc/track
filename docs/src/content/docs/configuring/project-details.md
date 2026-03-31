---
title: Confirm Project Details
description: Check the repo metadata that remote dispatch depends on before your first real run.
sidebar:
  order: 3
---

Open **Projects** in the WebUI, select each project you care about, and verify the metadata in **Project details**.

For remote dispatch, these three fields matter:

- **Repo URL**
- **Git URL**
- **Base branch**: the branch that `track` uses as the base for agent-opened pull requests. In the common case this matches the repository's default branch, but you can override it when your real integration branch is something longer-lived such as `develop`.

The optional **Description** field is just for human context.

## Why this matters

`track project register` tries to infer metadata from your local checkout, but the remote runner depends on this data being correct when it prepares worktrees and opens pull requests. A small mistake here is enough to make the first dispatch feel broken even when everything else is configured correctly.

## Good default examples

- Repo URL: `https://github.com/acme/project-a`
- Git URL: `git@github.com:acme/project-a.git`
- Base branch: `main`

Once these fields look right, you are ready to use the normal task and review flows.
