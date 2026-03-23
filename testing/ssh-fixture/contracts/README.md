# Mock State Files

The fixture uses declarative JSON files instead of hard-coded scenarios so test
cases can reconfigure behavior without rebuilding the image.

## `gh.json`

Top-level fields:

- `login`
  GitHub login returned by `gh api user --jq .login`.
- `repositories`
  Map keyed by canonical repo URL, for example
  `https://github.com/acme/project-a`.

Each repository entry contains:

- `name`
  Short repository name, for example `project-a`.
- `upstreamBarePath`
  Absolute in-container path to the seeded upstream bare repo.
- `forkOwner`
  Owner name used when the mock resolves `owner/repo` lookups.
- `forkBarePath`
  Absolute in-container path where `gh repo fork` should create the fork.

## `codex.json`

Top-level fields:

- `mode`
  One of `success`, `blocked`, `hang`, or `error`.
- `sleepSeconds`
  Optional delay before returning a terminal result.
- `status`
  Structured terminal status to write for `success` and `blocked`.
- `summary`
  Human-readable summary to place in `result.json`.
- `pullRequestUrl`
  Optional PR URL to return.
- `branchName`
  Optional branch name override. When omitted, the mock uses the current git
  branch in the prepared worktree.
- `worktreePath`
  Optional worktree path override. When omitted, the mock uses the `-C` path.
- `notes`
  Optional extra notes.
- `createCommit`
  Optional object that asks the mock to make a real git commit in the worktree.

`createCommit` fields:

- `message`
  Commit message to use.
- `files`
  Array of files to create or overwrite before committing.

Each file entry contains:

- `path`
  Relative path inside the worktree.
- `contents`
  File contents to write.
