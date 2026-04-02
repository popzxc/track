from pathlib import Path
import json
import textwrap

from .constants import REPO_ROOT
from .shell_utils import make_executable, write_text
from .smoke_context import SmokeContext


# ==============================================================================
# Host-Mode Shim Installation
# ==============================================================================
#
# The macOS smoke still wants to exercise the installed `track-backend` wrapper
# unchanged, but that runner does not give us a real Docker daemon. We therefore
# install narrow, explicit shims into a temp bin directory and prepend that
# directory to PATH only for the smoke commands that need them.


def install_macos_host_shims(context: SmokeContext) -> None:
    context.shim_bin_dir.mkdir(parents=True, exist_ok=True)
    context.shim_state_dir.mkdir(parents=True, exist_ok=True)
    context.remote_home_dir.mkdir(parents=True, exist_ok=True)
    context.remote_bin_dir.mkdir(parents=True, exist_ok=True)

    shim_entry = REPO_ROOT / "testing" / "full_ci_smoke" / "shim.py"
    if not shim_entry.is_file():
        raise RuntimeError(f"Expected shim entrypoint at {shim_entry}.")

    for command_name in ["docker", "ssh", "scp"]:
        wrapper_path = context.shim_bin_dir / command_name
        write_text(
            wrapper_path,
            textwrap.dedent(
                f"""\
                #!/usr/bin/env bash
                set -euo pipefail
                exec python3 {shell_quote(str(shim_entry))} {shell_quote(command_name)} "$@"
                """
            ),
        )
        make_executable(wrapper_path)

    install_remote_tool_wrapper(
        context,
        command_name="gh",
        mock_script=REPO_ROOT / "testing" / "ssh-fixture" / "mocks" / "gh.py",
    )
    install_remote_tool_wrapper(
        context,
        command_name="codex",
        mock_script=REPO_ROOT / "testing" / "ssh-fixture" / "mocks" / "codex.py",
    )
    install_remote_tool_wrapper(
        context,
        command_name="claude",
        mock_script=REPO_ROOT / "testing" / "ssh-fixture" / "mocks" / "claude.py",
    )


def install_remote_tool_wrapper(
    context: SmokeContext,
    *,
    command_name: str,
    mock_script: Path,
) -> None:
    if not mock_script.is_file():
        raise RuntimeError(f"Expected remote mock script at {mock_script}.")

    wrapper_path = context.remote_bin_dir / command_name
    write_text(
        wrapper_path,
        textwrap.dedent(
            f"""\
            #!/usr/bin/env bash
            set -euo pipefail
            export TRACK_TESTING_RUNTIME_DIR={shell_quote(str(context.fixture_runtime_dir))}
            exec python3 {shell_quote(str(mock_script))} "$@"
            """
        ),
    )
    make_executable(wrapper_path)


# ==============================================================================
# Host-Mode Fixture Layout
# ==============================================================================
#
# The Linux fixture container normalizes its runtime directory on startup. The
# host-mode variant needs the same layout and default state files before the
# SSH/SCP shims start executing remote scripts locally.


def ensure_host_fixture_layout(context: SmokeContext) -> None:
    for relative_path in [
        "state",
        "logs",
        "git",
    ]:
        (context.fixture_runtime_dir / relative_path).mkdir(parents=True, exist_ok=True)

    (context.remote_home_dir / ".ssh").mkdir(parents=True, exist_ok=True)

    ensure_default_state(
        context.fixture_runtime_dir / "state" / "gh.json",
        {"login": "fixture-user", "repositories": {}},
    )
    ensure_default_state(
        context.fixture_runtime_dir / "state" / "codex.json",
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
    ensure_default_state(
        context.fixture_runtime_dir / "state" / "claude.json",
        {
            "mode": "success",
            "sleepSeconds": 0,
            "status": "succeeded",
            "summary": "Mock Claude completed successfully.",
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


def ensure_default_state(path: Path, payload: dict) -> None:
    if path.exists():
        return

    write_text(path, json.dumps(payload, indent=2) + "\n")


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"
