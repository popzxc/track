import json
import sqlite3
import shutil
import sys

from ..api_client import TrackApiClient
from ..constants import FIXTURECTL_PATH, REPO_ROOT
from ..shell_utils import run
from ..smoke_context import SmokeContext


# ==============================================================================
# Diagnostics, Cleanup, And Summary Output
# ==============================================================================
#
# The smoke needs useful logs when it fails, but those details should not
# obscure the happy-path actions. Keeping diagnostics and teardown together also
# makes it easier to verify that each platform variant cleans up the same way.


def print_failure_diagnostics(context: SmokeContext) -> None:
    print("\nSmoke test failed. Collecting diagnostics...", file=sys.stderr)
    print_backend_remote_agent_config(context)
    print_latest_dispatch_diagnostic(context)
    if context.track_backend_path.is_file():
        try:
            run(
                [str(context.track_backend_path), "ps"],
                cwd=REPO_ROOT,
                env=context.smoke_env(),
                check=False,
            )
            run(
                [str(context.track_backend_path), "logs", "--no-color"],
                cwd=REPO_ROOT,
                env=context.smoke_env(),
                check=False,
            )
        except Exception as error:  # noqa: BLE001
            print(f"Could not collect backend logs: {error}", file=sys.stderr)

    runtime_logs = context.temp_root / "fixture-runtime" / "logs"
    if runtime_logs.exists():
        for log_path in sorted(runtime_logs.rglob("*")):
            if not log_path.is_file():
                continue
            print(f"\n--- {log_path} ---", file=sys.stderr)
            try:
                print(log_path.read_text(encoding="utf-8"), file=sys.stderr)
            except Exception as error:  # noqa: BLE001
                print(f"Could not read {log_path}: {error}", file=sys.stderr)


def print_backend_remote_agent_config(context: SmokeContext) -> None:
    database_path = context.home_dir / ".track" / "backend" / "track.sqlite"
    if not database_path.is_file():
        return

    print(f"\n--- {database_path} :: remote_agent_config ---", file=sys.stderr)
    try:
        connection = sqlite3.connect(database_path)
        try:
            row = connection.execute(
                "SELECT setting_json FROM backend_settings WHERE setting_key = ?",
                ("remote_agent_config",),
            ).fetchone()
        finally:
            connection.close()
    except Exception as error:  # noqa: BLE001
        print(f"Could not inspect backend settings: {error}", file=sys.stderr)
        return

    if row is None:
        print("(missing)", file=sys.stderr)
        return

    try:
        rendered = json.dumps(json.loads(row[0]), indent=2)
    except Exception:  # noqa: BLE001
        rendered = str(row[0])
    print(rendered, file=sys.stderr)


def print_latest_dispatch_diagnostic(context: SmokeContext) -> None:
    if context.api_base_url is None or context.task_id is None:
        return

    print("\n--- latest dispatch record ---", file=sys.stderr)
    try:
        dispatch = TrackApiClient(context.api_base_url).latest_dispatch_for_task(task_id=context.task_id)
    except Exception as error:  # noqa: BLE001
        print(f"Could not read latest dispatch record: {error}", file=sys.stderr)
        return

    if dispatch is None:
        print("(missing)", file=sys.stderr)
        return

    print(json.dumps(dispatch, indent=2), file=sys.stderr)


def cleanup_environment(context: SmokeContext) -> None:
    if context.backend_running and context.track_backend_path.is_file():
        run(
            [str(context.track_backend_path), "down", "-v", "--remove-orphans"],
            cwd=REPO_ROOT,
            env=context.backend_env(),
            check=False,
        )

    if (
        context.fixture_running
        and context.fixture_container_name is not None
        and context.install_flow_options is not None
        and context.install_flow_options.platform == "linux-docker"
    ):
        run(
            [
                "python3",
                str(FIXTURECTL_PATH),
                "stop",
                "--name",
                context.fixture_container_name,
            ],
            cwd=REPO_ROOT,
            env=context.host_tool_env(),
            check=False,
        )

    if context.temp_root.exists():
        shutil.rmtree(context.temp_root, ignore_errors=True)


def print_install_flow_summary(context: SmokeContext) -> None:
    print("\nInstall smoke flow completed successfully.")
    print(f"Task: {context.task_id}")
    print(f"Dispatch: {context.dispatch_id}")
    print(f"Review: {context.review_id}")
