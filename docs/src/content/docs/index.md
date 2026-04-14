---
title: track Docs
description: Learn track from first setup through remote dispatch, PR reviews, and contributor workflows.
template: splash
sidebar:
  hidden: true
hero:
  title: track
  tagline: Capture rough notes as tasks, run remote coding sessions, and keep project work moving from one local tool.
  image:
    file: ../../assets/track-docs-mark.svg
    alt: Stylized open book mark inspired by the track UI.
  actions:
    - text: Start Initial Setup
      link: ./initial-setup/intro/
      icon: right-arrow
    - text: Jump to Reference
      link: ./reference/config-files/
      variant: minimal
---

`track` keeps the everyday loop in one place:

- capture a task quickly from the CLI
- refine, dispatch, and follow up from the local WebUI
- run remote work through Codex or Claude when the task is ready

The docs are organized around the order most people need:

- **Initial Setup** explains the model, security boundaries, and the steps to prepare both the local machine and the remote runner.
- **Configuring** covers project registration, runner settings, and project metadata.
- **Using WebUI** walks through dispatches, follow-ups, PR reviews, runner choice, and common workflows.
- **Reference** is the place to look up files, commands, and settings.
- **Development Flow** is for contributors who need the repo shape and local workflow.

If you are coming from older notes or screenshots, keep in mind that current installs store live state in the backend database and in `~/.config/track/cli.json`.
