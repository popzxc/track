import json
import shlex
import socket
import stat
import subprocess
import time
from pathlib import Path


class CommandFailure(RuntimeError):
    pass


def render_command(command: list[str]) -> str:
    return " ".join(shlex.quote(part) for part in command)


def run(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    print(f"$ {render_command(command)}")
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=capture_output,
    )
    if check and completed.returncode != 0:
        stdout_text = (completed.stdout or "").strip()
        stderr_text = (completed.stderr or "").strip()
        raise CommandFailure(
            "\n\n".join(
                part
                for part in [
                    f"Command failed with exit code {completed.returncode}: {render_command(command)}",
                    stdout_text and f"stdout:\n{stdout_text}",
                    stderr_text and f"stderr:\n{stderr_text}",
                ]
                if part
            )
        )
    if capture_output and completed.stdout and completed.stdout.strip():
        print(completed.stdout.strip())
    return completed


def run_json(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
) -> dict:
    completed = run(command, cwd=cwd, env=env, capture_output=True)
    return json.loads(completed.stdout)


def reserve_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        sock.listen()
        return int(sock.getsockname()[1])


def wait_until(
    description: str,
    callback,
    *,
    timeout_seconds: float,
    interval_seconds: float = 1.0,
):
    deadline = time.monotonic() + timeout_seconds
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            result = callback()
            if result:
                return result
        except Exception as error:  # noqa: BLE001
            last_error = error
        time.sleep(interval_seconds)

    if last_error is not None:
        raise RuntimeError(f"Timed out while waiting for {description}: {last_error}") from last_error
    raise RuntimeError(f"Timed out while waiting for {description}.")


def write_text(path: Path, contents: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")


def make_executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
