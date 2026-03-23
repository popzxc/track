#!/usr/bin/env python3

import subprocess
import sys
from pathlib import Path
from typing import Any

from common import advisory_lock, append_jsonl, load_json, log_path, state_path, utc_timestamp


def load_state() -> dict[str, Any]:
    return load_json(
        state_path("gh"),
        {
            "login": "fixture-user",
            "repositories": {},
        },
    )


def repository_by_target(state: dict[str, Any], target: str) -> dict[str, Any] | None:
    owner, _, name = target.partition("/")
    if not owner or not name:
        return None

    for entry in state.get("repositories", {}).values():
        if entry.get("forkOwner") == owner and entry.get("name") == name:
            return entry

    return None


def log_invocation(argv: list[str], result: dict[str, Any]) -> None:
    append_jsonl(
        log_path("gh"),
        {
            "argv": argv,
            "cwd": str(Path.cwd()),
            "result": result,
            "timestamp": utc_timestamp(),
        },
    )


def clone_bare_repository(source: str, destination: str) -> None:
    # Two dispatches for the same repository can ask `gh repo fork` to create
    # the same bare fork at nearly the same time. We serialize by destination so
    # the live tests exercise the app's concurrency behavior instead of a mock-
    # specific clone race.
    with advisory_lock(f"gh-fork:{destination}"):
        destination_path = Path(destination)
        if destination_path.exists():
            return

        destination_path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["git", "clone", "--bare", source, destination],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def main(argv: list[str]) -> int:
    state = load_state()

    try:
        # TODO: Expand this dispatcher only when production code starts relying
        # on additional `gh` subcommands. Keeping it narrow makes new
        # dependencies on `gh` visible in tests instead of silently accepted.
        if argv[:3] == ["api", "user", "--jq"]:
            if argv[3:] != [".login"]:
                raise ValueError(f"Unsupported gh api invocation: {argv}")

            login = state.get("login", "").strip()
            if not login:
                raise ValueError("Mock gh state must provide a non-empty login.")

            print(login)
            log_invocation(argv, {"exitCode": 0, "login": login})
            return 0

        if len(argv) >= 5 and argv[0] == "repo" and argv[1] == "view":
            target = argv[2]
            repository = repository_by_target(state, target)
            if repository is None:
                log_invocation(argv, {"exitCode": 1, "target": target, "reason": "not-found"})
                return 1

            ssh_url = repository.get("forkBarePath", "").strip()
            if not ssh_url:
                raise ValueError(f"Mock gh repository entry for {target} is missing forkBarePath.")
            if not Path(ssh_url).exists():
                log_invocation(
                    argv,
                    {
                        "exitCode": 1,
                        "target": target,
                        "reason": "fork-not-created-yet",
                        "sshUrl": ssh_url,
                    },
                )
                return 1

            print(ssh_url)
            log_invocation(argv, {"exitCode": 0, "target": target, "sshUrl": ssh_url})
            return 0

        if len(argv) >= 3 and argv[0] == "repo" and argv[1] == "fork":
            repo_url = argv[2]
            repository = state.get("repositories", {}).get(repo_url)
            if repository is None:
                raise ValueError(f"Mock gh state does not define repository {repo_url}.")

            upstream_bare_path = repository.get("upstreamBarePath", "").strip()
            fork_bare_path = repository.get("forkBarePath", "").strip()
            if not upstream_bare_path or not fork_bare_path:
                raise ValueError(
                    f"Mock gh repository entry for {repo_url} must define upstreamBarePath and forkBarePath."
                )

            clone_bare_repository(upstream_bare_path, fork_bare_path)
            log_invocation(
                argv,
                {
                    "exitCode": 0,
                    "forkBarePath": fork_bare_path,
                    "repoUrl": repo_url,
                    "upstreamBarePath": upstream_bare_path,
                },
            )
            return 0

        raise ValueError(f"Unsupported gh invocation: {argv}")
    except Exception as error:
        message = str(error).strip() or error.__class__.__name__
        print(message, file=sys.stderr)
        log_invocation(argv, {"exitCode": 2, "error": message})
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
