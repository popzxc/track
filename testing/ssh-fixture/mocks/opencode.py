#!/usr/bin/env python3

import json
import signal
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from common import append_jsonl, ensure_parent, load_json, log_path, state_path, utc_timestamp


class TerminatedBySignal(Exception):
    pass


def handle_sigterm(_signum: int, _frame: Any) -> None:
    raise TerminatedBySignal()


def load_state() -> dict[str, Any]:
    return load_json(
        state_path("opencode"),
        {
            "mode": "success",
            "sleepSeconds": 0,
            "status": "succeeded",
            "summary": "Mock opencode completed successfully.",
            "pullRequestUrl": None,
            "branchName": None,
            "worktreePath": None,
            "reviewSubmitted": False,
            "githubReviewId": None,
            "githubReviewUrl": None,
            "reviewBody": None,
            "notes": None,
        },
    )


def parse_opencode_args(argv: list[str]) -> dict[str, Any]:
    worktree_path = Path.cwd()
    format_type = "default"

    index = 0
    while index < len(argv):
        argument = argv[index]
        if argument == "--add-dir" and index + 1 < len(argv):
            worktree_path = Path(argv[index + 1])
            index += 2
            continue
        if argument == "--format" and index + 1 < len(argv):
            format_type = argv[index + 1]
            index += 2
            continue
        index += 1

    return {
        "format": format_type,
        "worktree_path": worktree_path,
    }


def log_invocation(argv: list[str], result: dict[str, Any]) -> None:
    append_jsonl(
        log_path("opencode"),
        {
            "argv": argv,
            "cwd": str(Path.cwd()),
            "result": result,
            "timestamp": utc_timestamp(),
        },
    )


def current_branch_name(worktree_path: Path) -> str:
    completed = subprocess.run(
        ["git", "-C", str(worktree_path), "rev-parse", "--abbrev-ref", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def configure_git_identity(worktree_path: Path) -> None:
    subprocess.run(
        ["git", "-C", str(worktree_path), "config", "user.name", "Track Testing"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        ["git", "-C", str(worktree_path), "config", "user.email", "track-testing@example.com"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def apply_mock_changes(worktree_path: Path, create_commit: dict[str, Any] | None) -> None:
    if not create_commit:
        return

    files = create_commit.get("files", [])
    for file_entry in files:
        relative_path = Path(file_entry["path"])
        destination = worktree_path / relative_path
        ensure_parent(destination)
        destination.write_text(file_entry["contents"], encoding="utf-8")

    configure_git_identity(worktree_path)
    subprocess.run(
        ["git", "-C", str(worktree_path), "add", "--all"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        [
            "git",
            "-C",
            str(worktree_path),
            "commit",
            "--allow-empty",
            "-m",
            create_commit.get("message", "chore: apply opencode fixture change"),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def parse_pull_request_reference(pull_request_url: str) -> tuple[str, str, str]:
    trimmed = pull_request_url.strip().rstrip("/")
    parts = [part for part in trimmed.split("/") if part]
    if len(parts) < 5 or parts[-2] != "pull":
        raise ValueError(f"Unsupported pull request URL in mock opencode state: {pull_request_url}")

    return parts[-4], parts[-3], parts[-1]


def pull_request_url_from_prompt(prompt: str) -> str | None:
    for line in prompt.splitlines():
        prefix = "- Pull request: "
        if line.startswith(prefix):
            return line[len(prefix):].strip() or None

    return None


def submit_mock_review(prompt: str, state: dict[str, Any]) -> dict[str, Any]:
    pull_request_url = (state.get("pullRequestUrl") or pull_request_url_from_prompt(prompt) or "").strip()
    if not pull_request_url:
        raise ValueError("Mock opencode review submission requires a pull request URL.")

    owner, repository_name, number = parse_pull_request_reference(pull_request_url)
    review_body = (
        state.get("reviewBody")
        or "@octocat requested me to review this PR.\n\nMock review body from the fixture."
    )
    review_event = (state.get("reviewEvent") or "COMMENT").strip().upper() or "COMMENT"
    endpoint = f"repos/{owner}/{repository_name}/pulls/{number}/reviews"
    completed = subprocess.run(
        [
            "gh",
            "api",
            "--method",
            "POST",
            endpoint,
            "-f",
            f"body={review_body}",
            "-f",
            f"event={review_event}",
        ],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return json.loads(completed.stdout)


def emit_event(event_type: str, part_payload: dict[str, Any]) -> None:
    """Emit one line of the opencode JSON event stream."""
    event = {
        "type": event_type,
        "timestamp": int(time.time() * 1000),
        "sessionID": "ses_mock_opencode_session",
        "part": {
            "id": f"prt_mock_{event_type}",
            "sessionID": "ses_mock_opencode_session",
            **part_payload,
        },
    }
    print(json.dumps(event), flush=True)


def main(argv: list[str]) -> int:
    prompt = sys.stdin.read()
    state = load_state()

    try:
        parsed_args = parse_opencode_args(argv)
        worktree_path = parsed_args["worktree_path"]
        format_type = parsed_args["format"]
        
        if format_type != "json":
            raise ValueError(f"Mock opencode requires --format json, got: {format_type}")

        sleep_seconds = int(state.get("sleepSeconds", 0) or 0)
        if sleep_seconds > 0:
            time.sleep(sleep_seconds)

        mode = (state.get("mode") or "success").strip()
        if mode == "hang":
            while True:
                time.sleep(1)

        if mode == "error":
            raise RuntimeError(state.get("summary") or "Mock opencode failed deliberately.")

        # Emit event stream
        # Step 1: Start
        emit_event("step_start", {
            "messageID": "msg_mock_step1",
            "snapshot": "mock_snapshot_1",
            "type": "step-start",
        })

        # Apply changes if configured
        apply_mock_changes(worktree_path, state.get("createCommit"))
        
        # Step 1: Finish (tool calls)
        emit_event("step_finish", {
            "reason": "tool-calls",
            "snapshot": "mock_snapshot_1",
            "messageID": "msg_mock_step1",
            "type": "step-finish",
            "tokens": {"total": 1000, "input": 900, "output": 100, "reasoning": 0, "cache": {"write": 0, "read": 0}},
        })

        # Step 2: Start (final response)
        emit_event("step_start", {
            "messageID": "msg_mock_step2",
            "snapshot": "mock_snapshot_1",
            "type": "step-start",
        })

        # Build result payload
        if "pullRequestUrl" not in prompt or "review" not in prompt.lower():
            # Dispatch outcome
            result_payload = {
                "status": state.get("status", "succeeded"),
                "summary": state.get("summary", "Mock opencode completed successfully."),
                "pullRequestUrl": state.get("pullRequestUrl"),
                "branchName": state.get("branchName") or current_branch_name(worktree_path),
                "worktreePath": state.get("worktreePath") or str(worktree_path),
                "notes": state.get("notes"),
            }
        else:
            # Review outcome
            review_submitted = bool(state.get("reviewSubmitted", False))
            review_payload: dict[str, Any] | None = None
            if review_submitted:
                review_payload = submit_mock_review(prompt, state)

            result_payload = {
                "status": state.get("status", "succeeded"),
                "summary": state.get("summary", "Mock opencode completed the review successfully."),
                "worktreePath": state.get("worktreePath") or str(worktree_path),
                "notes": state.get("notes"),
                "reviewSubmitted": review_submitted,
                "githubReviewId": (
                    state.get("githubReviewId")
                    or (str(review_payload.get("id")) if review_payload and review_payload.get("id") is not None else None)
                ) if review_submitted else None,
                "githubReviewUrl": (
                    state.get("githubReviewUrl")
                    or (review_payload.get("html_url") if review_payload else None)
                ) if review_submitted else None,
            }

        # Emit text event with JSON result
        emit_event("text", {
            "messageID": "msg_mock_step2",
            "type": "text",
            "text": json.dumps(result_payload),
            "time": {"start": int(time.time() * 1000), "end": int(time.time() * 1000)},
        })

        # Step 2: Finish (stop)
        emit_event("step_finish", {
            "reason": "stop",
            "snapshot": "mock_snapshot_1",
            "messageID": "msg_mock_step2",
            "type": "step-finish",
            "tokens": {"total": 1100, "input": 1000, "output": 100, "reasoning": 0, "cache": {"write": 0, "read": 0}},
        })

        log_invocation(
            argv,
            {
                "exitCode": 0,
                "mode": mode,
                "promptLength": len(prompt),
                "worktreePath": str(worktree_path),
            },
        )
        return 0
    except TerminatedBySignal:
        log_invocation(argv, {"exitCode": 130, "mode": "terminated", "signal": "TERM"})
        return 130
    except KeyboardInterrupt:
        log_invocation(argv, {"exitCode": 130, "mode": "hang", "signal": "KeyboardInterrupt"})
        return 130
    except Exception as error:
        message = str(error).strip() or error.__class__.__name__
        print(message, file=sys.stderr)
        log_invocation(argv, {"error": message, "exitCode": 2})
        return 2


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, handle_sigterm)
    raise SystemExit(main(sys.argv[1:]))
