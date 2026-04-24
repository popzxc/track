import hashlib
import os
import fcntl
import shutil
import signal
import subprocess
import tempfile
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

STATUS_FILE_NAME = "status.txt"
RESULT_FILE_NAME = "result.json"
STDERR_FILE_NAME = "stderr.log"
FINISHED_AT_FILE_NAME = "finished-at.txt"
PROMPT_FILE_NAME = "prompt.md"
SCHEMA_FILE_NAME = "result-schema.json"
LAUNCHER_PID_FILE_NAME = "launcher.pid"
# Agent PID file names are now tool-specific and chosen at runtime.
CODEX_EVENTS_FILE_NAME = "events.jsonl"
REVIEW_WORKTREE_DIRECTORY_NAME = "review-worktrees"
REVIEW_RUN_DIRECTORY_NAME = "review-runs"

TOOL_OVERRIDE_ENV_VARS = {
    "gh": "TRACK_REMOTE_HELPER_GH",
    "codex": "TRACK_REMOTE_HELPER_CODEX",
    "claude": "TRACK_REMOTE_HELPER_CLAUDE",
}
LOCK_ROOT = Path(
    os.environ.get("TRACK_REMOTE_HELPER_LOCK_ROOT", tempfile.gettempdir())
) / "track-remote-helper-locks"


class CommandError(RuntimeError):
    pass


def lock_path(name: str) -> Path:
    digest = hashlib.sha256(name.encode("utf-8")).hexdigest()
    return LOCK_ROOT / f"{digest}.lock"


@contextmanager
def advisory_lock(name: str):
    path = lock_path(name)
    ensure_parent(path)
    with path.open("w", encoding="utf-8") as handle:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def expand_remote_path(raw_path: str) -> Path:
    return Path(os.path.expanduser(raw_path))


def utc_timestamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def write_text(path: Path, contents: str) -> None:
    ensure_parent(path)
    path.write_text(contents, encoding="utf-8")


def write_text_with_trailing_newline(path: Path, contents: str) -> None:
    write_text(path, f"{contents}\n")


def read_optional_text(path: Path) -> Optional[str]:
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def remove_path(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink(missing_ok=True)
        return

    if path.is_dir():
        shutil.rmtree(path)


def resolve_binary(tool_name: str) -> str:
    return os.environ.get(TOOL_OVERRIDE_ENV_VARS.get(tool_name, ""), tool_name)


def check_command(
    argv: list[str],
    *,
    cwd: Optional[Path] = None,
    env: Optional[dict[str, str]] = None,
    capture_output: bool = False,
) -> subprocess.CompletedProcess:
    completed = subprocess.run(
        argv,
        cwd=str(cwd) if cwd is not None else None,
        env=env,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode == 0:
        return completed

    stderr = (completed.stderr or "").strip()
    stdout = (completed.stdout or "").strip()
    message = stderr or stdout or f"{argv[0]} exited with status {completed.returncode}"
    raise CommandError(message)


def command_succeeds(
    argv: list[str],
    *,
    cwd: Optional[Path] = None,
) -> bool:
    completed = subprocess.run(
        argv,
        cwd=str(cwd) if cwd is not None else None,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    return completed.returncode == 0


def read_pid(path: Path) -> Optional[int]:
    contents = read_optional_text(path)
    if contents is None:
        return None

    trimmed = contents.strip()
    if not trimmed:
        return None

    try:
        return int(trimmed)
    except ValueError:
        return None


def kill_if_running(pid: Optional[int]) -> None:
    if pid is None:
        return

    try:
        os.kill(pid, 0)
    except OSError:
        return

    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
