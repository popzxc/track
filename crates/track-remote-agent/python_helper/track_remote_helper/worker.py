import json
import os
import signal
import subprocess
from pathlib import Path
from typing import Any, Optional

from track_remote_helper.common import (
    AGENT_PID_FILE_NAME,
    CODEX_EVENTS_FILE_NAME,
    FINISHED_AT_FILE_NAME,
    LAUNCHER_PID_FILE_NAME,
    PROMPT_FILE_NAME,
    RESULT_FILE_NAME,
    SCHEMA_FILE_NAME,
    STATUS_FILE_NAME,
    STDERR_FILE_NAME,
    resolve_binary,
    utc_timestamp,
    write_text_with_trailing_newline,
)


def run_worker_from_config(config_path: Path) -> int:
    config = json.loads(config_path.read_text(encoding="utf-8"))
    run_directory = Path(config["runDirectory"])
    worktree_path = Path(config["worktreePath"])
    preferred_tool = str(config["preferredTool"])
    shell_prelude = str(config.get("shellPrelude") or "")

    prompt_path = run_directory / PROMPT_FILE_NAME
    schema_path = run_directory / SCHEMA_FILE_NAME
    result_path = run_directory / RESULT_FILE_NAME
    stderr_path = run_directory / STDERR_FILE_NAME
    status_path = run_directory / STATUS_FILE_NAME
    finished_at_path = run_directory / FINISHED_AT_FILE_NAME
    launcher_pid_path = run_directory / LAUNCHER_PID_FILE_NAME
    agent_pid_path = run_directory / AGENT_PID_FILE_NAME
    events_path = run_directory / CODEX_EVENTS_FILE_NAME

    write_text_with_trailing_newline(launcher_pid_path, str(os.getpid()))

    child: Optional[subprocess.Popen] = None

    def cancel_run(_signum: int, _frame: Any) -> None:
        nonlocal child
        if child is not None and child.poll() is None:
            try:
                child.terminate()
            except OSError:
                pass
        write_text_with_trailing_newline(status_path, "canceled")
        write_text_with_trailing_newline(finished_at_path, utc_timestamp())
        raise SystemExit(130)

    signal.signal(signal.SIGTERM, cancel_run)
    signal.signal(signal.SIGINT, cancel_run)

    write_text_with_trailing_newline(status_path, "running")

    with prompt_path.open("r", encoding="utf-8") as prompt_file, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr_file:
        child = spawn_agent_process(
            preferred_tool=preferred_tool,
            run_directory=run_directory,
            worktree_path=worktree_path,
            prompt_file=prompt_file,
            schema_path=schema_path,
            result_path=result_path,
            stderr_file=stderr_file,
            events_path=events_path,
            shell_prelude=shell_prelude,
        )
        write_text_with_trailing_newline(agent_pid_path, str(child.pid))
        return_code = child.wait()

    if return_code == 0:
        write_text_with_trailing_newline(status_path, "completed")
    else:
        current_status = (status_path.read_text(encoding="utf-8") if status_path.exists() else "").strip()
        if current_status != "canceled" and return_code != 130:
            write_text_with_trailing_newline(status_path, "launcher_failed")

    write_text_with_trailing_newline(finished_at_path, utc_timestamp())
    return 0


def spawn_agent_process(
    *,
    preferred_tool: str,
    run_directory: Path,
    worktree_path: Path,
    prompt_file,
    schema_path: Path,
    result_path: Path,
    stderr_file,
    events_path: Path,
    shell_prelude: str,
) -> subprocess.Popen:
    if preferred_tool == "claude":
        schema_content = schema_path.read_text(encoding="utf-8").replace("\n", "")
        tool_args = [
            resolve_binary("claude"),
            "-p",
            "--dangerously-skip-permissions",
            "--add-dir",
            str(worktree_path),
            "--output-format",
            "json",
            "--json-schema",
            schema_content,
        ]
        stdout_file = result_path.open("w", encoding="utf-8")
        cwd = worktree_path
    else:
        tool_args = [
            resolve_binary("codex"),
            "--search",
            "exec",
            "--dangerously-bypass-approvals-and-sandbox",
            "-C",
            str(worktree_path),
            "--json",
            "--output-schema",
            str(schema_path),
            "-o",
            str(result_path),
            "-",
        ]
        stdout_file = events_path.open("w", encoding="utf-8")
        cwd = run_directory

    spawn_args = tool_args
    if shell_prelude.strip():
        spawn_args = [
            "bash",
            "-lc",
            f"set -e\n{shell_prelude.rstrip()}\nset -eu\nexec \"$@\"\n",
            "track-remote-helper",
            *tool_args,
        ]

    try:
        return subprocess.Popen(
            spawn_args,
            cwd=str(cwd),
            stdin=prompt_file,
            stdout=stdout_file,
            stderr=stderr_file,
            text=True,
        )
    finally:
        stdout_file.close()
