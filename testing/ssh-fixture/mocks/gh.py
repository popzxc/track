#!/usr/bin/env python3

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

from common import (
    advisory_lock,
    append_jsonl,
    load_json,
    log_path,
    state_path,
    utc_timestamp,
    write_json,
)


def load_state() -> dict[str, Any]:
    return load_json(
        state_path("gh"),
        {
            "login": "fixture-user",
            "repositories": {},
        },
    )


def save_state(state: dict[str, Any]) -> None:
    write_json(state_path("gh"), state)


def repository_by_target(state: dict[str, Any], target: str) -> dict[str, Any] | None:
    owner, _, name = target.partition("/")
    if not owner or not name:
        return None

    for entry in state.get("repositories", {}).values():
        if entry.get("forkOwner") == owner and entry.get("name") == name:
            return entry

    return None


def repository_by_full_name(
    state: dict[str, Any],
    owner: str,
    repository_name: str,
) -> dict[str, Any] | None:
    for repo_url, entry in state.get("repositories", {}).items():
        entry_owner = entry.get("owner")
        if not entry_owner:
            entry_owner = repo_url.rstrip("/").split("/")[-2]

        if entry_owner == owner and entry.get("name") == repository_name:
            return entry

    return None


def parse_api_invocation(argv: list[str]) -> tuple[str, str, dict[str, str]]:
    method = "GET"
    endpoint: str | None = None
    fields: dict[str, str] = {}

    index = 1
    while index < len(argv):
        argument = argv[index]
        if argument == "--method" and index + 1 < len(argv):
            method = argv[index + 1].upper()
            index += 2
            continue
        if argument == "-f" and index + 1 < len(argv):
            key, _, value = argv[index + 1].partition("=")
            if not key:
                raise ValueError(f"Unsupported gh api field argument: {argv[index + 1]}")
            fields[key] = value
            index += 2
            continue
        if argument.startswith("-"):
            raise ValueError(f"Unsupported gh api flag: {argument}")
        if endpoint is None:
            endpoint = argument
            index += 1
            continue

        raise ValueError(f"Unsupported gh api invocation: {argv}")

    if endpoint is None:
        raise ValueError(f"Unsupported gh api invocation, missing endpoint: {argv}")

    return method, endpoint, fields


def parse_pull_request_endpoint(
    endpoint: str,
) -> tuple[str, str, int, str, str | None]:
    path, _, query = endpoint.partition("?")
    parts = [part for part in path.split("/") if part]
    if len(parts) < 5 or parts[0] != "repos" or parts[3] not in {"pulls", "issues"}:
        raise ValueError(f"Unsupported gh api endpoint: {endpoint}")

    owner = parts[1]
    repository_name = parts[2]
    resource = parts[3]
    pull_request_number = int(parts[4])
    suffix = "/".join(parts[5:]) or None

    return (
        owner,
        repository_name,
        pull_request_number,
        resource,
        suffix if not query else f"{suffix}?{query}" if suffix else f"?{query}",
    )


def load_pull_request(
    state: dict[str, Any],
    owner: str,
    repository_name: str,
    pull_request_number: int,
) -> tuple[dict[str, Any], dict[str, Any]]:
    repository = repository_by_full_name(state, owner, repository_name)
    if repository is None:
        raise ValueError(f"Mock gh state does not define repository {owner}/{repository_name}.")

    pull_request = repository.get("pullRequests", {}).get(str(pull_request_number))
    if pull_request is None:
        raise ValueError(
            f"Mock gh state does not define pull request {pull_request_number} for {owner}/{repository_name}."
        )

    return repository, pull_request


def pull_request_api_payload(
    owner: str,
    repository_name: str,
    pull_request: dict[str, Any],
) -> dict[str, Any]:
    return {
        "number": pull_request["number"],
        "title": pull_request["title"],
        "state": pull_request.get("state", "open"),
        "merged_at": pull_request.get("mergedAt"),
        "base": {
            "ref": pull_request.get("baseRef", "main"),
        },
        "head": {
            "sha": pull_request.get("headSha", ""),
            "ref": pull_request.get("headRef", ""),
            "repo": {
                "full_name": f"{owner}/{repository_name}",
            },
        },
    }


def review_api_payload(review: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": review.get("id"),
        "html_url": review.get("html_url"),
        "body": review.get("body"),
        "state": review.get("state", "COMMENTED"),
        "submitted_at": review.get("submitted_at"),
        "user": {
            "login": review.get("user", {}).get("login", ""),
        },
    }


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


def handle_api_invocation(state: dict[str, Any], argv: list[str]) -> int:
    if argv[:3] == ["api", "user", "--jq"]:
        if argv[3:] != [".login"]:
            raise ValueError(f"Unsupported gh api invocation: {argv}")

        login = state.get("login", "").strip()
        if not login:
            raise ValueError("Mock gh state must provide a non-empty login.")

        print(login)
        log_invocation(argv, {"exitCode": 0, "login": login})
        return 0

    method, endpoint, fields = parse_api_invocation(argv)
    owner, repository_name, pull_request_number, resource, suffix = parse_pull_request_endpoint(endpoint)
    repository, pull_request = load_pull_request(
        state,
        owner,
        repository_name,
        pull_request_number,
    )

    if method == "GET" and resource == "pulls" and suffix is None:
        payload = pull_request_api_payload(owner, repository_name, pull_request)
        print(json.dumps(payload))
        log_invocation(
            argv,
            {
                "endpoint": endpoint,
                "exitCode": 0,
                "pullRequest": pull_request_number,
                "repository": f"{owner}/{repository_name}",
            },
        )
        return 0

    if method == "GET" and resource == "pulls" and suffix == "reviews?per_page=100":
        payload = [review_api_payload(review) for review in pull_request.get("reviews", [])]
        print(json.dumps(payload))
        log_invocation(
            argv,
            {
                "endpoint": endpoint,
                "exitCode": 0,
                "pullRequest": pull_request_number,
                "repository": f"{owner}/{repository_name}",
                "reviewsReturned": len(payload),
            },
        )
        return 0

    if method == "POST" and resource == "pulls" and suffix == "reviews":
        body = fields.get("body", "").strip()
        event = fields.get("event", "COMMENT").strip().upper() or "COMMENT"
        review_state = "COMMENTED" if event == "COMMENT" else event
        review_id = pull_request_number * 1000 + len(pull_request.get("reviews", [])) + 1
        review_entry = {
            "id": review_id,
            "html_url": f"https://github.com/{owner}/{repository_name}/pull/{pull_request_number}#pullrequestreview-{review_id}",
            "body": body,
            "event": event,
            "state": review_state,
            "submitted_at": utc_timestamp(),
            "user": {
                "login": state.get("login", "fixture-user"),
            },
        }
        pull_request.setdefault("reviews", []).append(review_entry)
        save_state(state)
        print(json.dumps(review_api_payload(review_entry)))
        log_invocation(
            argv,
            {
                "endpoint": endpoint,
                "event": event,
                "exitCode": 0,
                "pullRequest": pull_request_number,
                "repository": f"{owner}/{repository_name}",
                "reviewId": str(review_id),
                "reviewUrl": review_entry["html_url"],
                "reviewBody": body,
            },
        )
        return 0

    if method == "POST" and resource == "issues" and suffix == "comments":
        comment_body = fields.get("body", "").strip()
        comment_entry = {
            "body": comment_body,
            "created_at": utc_timestamp(),
            "user": {
                "login": state.get("login", "fixture-user"),
            },
        }
        pull_request.setdefault("comments", []).append(comment_entry)
        save_state(state)
        print(json.dumps(comment_entry))
        log_invocation(
            argv,
            {
                "commentBody": comment_body,
                "endpoint": endpoint,
                "exitCode": 0,
                "pullRequest": pull_request_number,
                "repository": f"{owner}/{repository_name}",
            },
        )
        return 0

    raise ValueError(f"Unsupported gh api invocation: {argv}")


def main(argv: list[str]) -> int:
    state = load_state()

    try:
        # TODO: Expand this dispatcher only when production code starts relying
        # on additional `gh` subcommands. Keeping it narrow makes new
        # dependencies on `gh` visible in tests instead of silently accepted.
        if argv and argv[0] == "api":
            return handle_api_invocation(state, argv)

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
