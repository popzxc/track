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
        state_path("codex"),
        {
            "mode": "success",
            "sleepSeconds": 0,
            "status": "succeeded",
            "summary": "Mock Codex completed successfully.",
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


def parse_codex_args(argv: list[str]) -> dict[str, Path]:
    worktree_path: Path | None = None
    output_path: Path | None = None
    schema_path: Path | None = None

    index = 0
    while index < len(argv):
        argument = argv[index]
        if argument == "-C" and index + 1 < len(argv):
            worktree_path = Path(argv[index + 1])
            index += 2
            continue
        if argument == "-o" and index + 1 < len(argv):
            output_path = Path(argv[index + 1])
            index += 2
            continue
        if argument == "--output-schema" and index + 1 < len(argv):
            schema_path = Path(argv[index + 1])
            index += 2
            continue
        index += 1

    if worktree_path is None or output_path is None or schema_path is None:
        raise ValueError(f"Unsupported codex invocation, missing required flags: {argv}")

    return {
        "output_path": output_path,
        "schema_path": schema_path,
        "worktree_path": worktree_path,
    }


def log_invocation(argv: list[str], result: dict[str, Any]) -> None:
    append_jsonl(
        log_path("codex"),
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
            create_commit.get("message", "chore: apply codex fixture change"),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def write_prompt_snapshot(run_directory: Path, prompt: str) -> None:
    prompt_snapshot_path = run_directory / "stdin-prompt.md"
    prompt_snapshot_path.write_text(prompt, encoding="utf-8")


def emit_event(event_type: str, payload: dict[str, Any]) -> None:
    print(json.dumps({"event": event_type, **payload}), flush=True)


def schema_properties(schema_path: Path) -> dict[str, Any]:
    return json.loads(schema_path.read_text(encoding="utf-8")).get("properties", {})


def parse_pull_request_reference(pull_request_url: str) -> tuple[str, str, str]:
    trimmed = pull_request_url.strip().rstrip("/")
    parts = [part for part in trimmed.split("/") if part]
    if len(parts) < 5 or parts[-2] != "pull":
        raise ValueError(f"Unsupported pull request URL in mock codex state: {pull_request_url}")

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
        raise ValueError("Mock Codex review submission requires a pull request URL.")

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


def main(argv: list[str]) -> int:
    prompt = sys.stdin.read()
    state = load_state()

    try:
        parsed_args = parse_codex_args(argv)
        worktree_path = parsed_args["worktree_path"]
        output_path = parsed_args["output_path"]
        schema_path = parsed_args["schema_path"]
        run_directory = output_path.parent

        if not schema_path.exists():
            raise ValueError(f"Expected output schema at {schema_path}, but it was not present.")

        schema = schema_properties(schema_path)
        write_prompt_snapshot(run_directory, prompt)

        emit_event("started", {"worktreePath": str(worktree_path)})
        sleep_seconds = int(state.get("sleepSeconds", 0) or 0)
        if sleep_seconds > 0:
            time.sleep(sleep_seconds)

        mode = (state.get("mode") or "success").strip()
        if mode == "hang":
            while True:
                time.sleep(1)

        if mode == "error":
            raise RuntimeError(state.get("summary") or "Mock Codex failed deliberately.")

        # TODO: Add richer event stream shaping when the app starts asserting on
        # `events.jsonl` contents rather than only terminal result files.
        apply_mock_changes(worktree_path, state.get("createCommit"))
        if "pullRequestUrl" not in schema:
            review_submitted = bool(state.get("reviewSubmitted", False))
            review_payload: dict[str, Any] | None = None
            if review_submitted:
                review_payload = submit_mock_review(prompt, state)

            result_payload = {
                "status": state.get("status", "succeeded"),
                "summary": state.get("summary", "Mock Codex completed the review successfully."),
                "worktreePath": state.get("worktreePath") or str(worktree_path),
                "notes": state.get("notes"),
            }
            if "reviewSubmitted" in schema:
                result_payload["reviewSubmitted"] = review_submitted
            if "githubReviewId" in schema:
                result_payload["githubReviewId"] = (
                    (
                        state.get("githubReviewId")
                        or (
                            str(review_payload.get("id"))
                            if review_payload and review_payload.get("id") is not None
                            else None
                        )
                    )
                    if review_submitted
                    else None
                )
            if "githubReviewUrl" in schema:
                result_payload["githubReviewUrl"] = (
                    (
                        state.get("githubReviewUrl")
                        or (review_payload.get("html_url") if review_payload else None)
                    )
                    if review_submitted
                    else None
                )
            if "reviewBody" in schema:
                result_payload["reviewBody"] = (
                    state.get("reviewBody")
                    or "@octocat requested me to review this PR.\n\nMock review body from the fixture."
                )
        else:
            result_payload = {
                "status": state.get("status", "succeeded"),
                "summary": state.get("summary", "Mock Codex completed successfully."),
                "pullRequestUrl": state.get("pullRequestUrl"),
                "branchName": state.get("branchName") or current_branch_name(worktree_path),
                "worktreePath": state.get("worktreePath") or str(worktree_path),
                "notes": state.get("notes"),
            }

        ensure_parent(output_path)
        output_path.write_text(json.dumps(result_payload, indent=2) + "\n", encoding="utf-8")
        emit_event("completed", {"status": result_payload["status"]})
        log_invocation(
            argv,
            {
                "exitCode": 0,
                "mode": mode,
                "outputPath": str(output_path),
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
