---
title: track Docs
description: Learn track from first setup through remote dispatch, PR reviews, and contributor workflows.
template: splash
sidebar:
  hidden: true
hero:
  title: track
  tagline: A gruvbox-dark book for setting up the CLI, configuring remote runs, using the WebUI, and understanding the project as a contributor.
  image:
    file: ../../assets/track-docs-mark.svg
    alt: Stylized open book mark inspired by the track UI.
  actions:
    - text: Start Initial Setup
      link: /initial-setup/local-and-remote-prerequisites/
      icon: right-arrow
    - text: Jump to Reference
      link: /reference/config-files/
      variant: minimal
---

`track` combines three pieces into one workflow:

- a local CLI that turns rough notes into tasks
- a local WebUI for editing, dispatching, and reviewing
- a remote runner that can use either Codex or Claude

This book is split into five parts:

- **Initial Setup** gets the local CLI, the WebUI, and the remote machine ready.
- **Configuring** covers the last mile: registering projects, saving runner settings, and checking project metadata.
- **Using WebUI** walks through dispatches, follow-ups, PR reviews, and runner choice.
- **Reference** is the lookup section for files, commands, and settings.
- **Development Flow** is aimed at developers and contributors who need the codebase shape and local workflow.

If you are coming from older notes or screenshots, treat this book as the source of truth. The current build stores its live state in the backend database and in `~/.config/track/cli.json`; older `config.json`-centric instructions are covered only in the reference section as legacy material.
