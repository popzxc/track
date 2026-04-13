import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Optional

from track_remote_helper.commands import handle_command
from track_remote_helper.common import CommandError
from track_remote_helper.worker import run_worker_from_config


def resolve_helper_path() -> Path:
    explicit_helper_path = os.environ.get("TRACK_REMOTE_HELPER_SELF", "").strip()
    if explicit_helper_path:
        return Path(explicit_helper_path).resolve()

    argv_helper_path = Path(sys.argv[0])
    if argv_helper_path.suffix == ".pyz" or argv_helper_path.is_file():
        return argv_helper_path.resolve()

    if sys.path:
        zipapp_path = Path(sys.path[0])
        if zipapp_path.suffix == ".pyz" and zipapp_path.exists():
            return zipapp_path.resolve()

    return argv_helper_path.resolve()


def main(argv: Optional[list[str]] = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if not args:
        print("expected a remote helper command", file=sys.stderr)
        return 2

    command_name = args[0]
    if command_name == "_run-worker":
        if len(args) != 2:
            print("expected a worker config path", file=sys.stderr)
            return 2
        return run_worker_from_config(Path(args[1]))

    if len(args) != 1:
        print(f"unexpected arguments for command {command_name!r}", file=sys.stderr)
        return 2

    try:
        request = json.load(sys.stdin)
        response = handle_command(command_name, request, resolve_helper_path())
    except json.JSONDecodeError as error:
        print(f"request body is not valid JSON: {error}", file=sys.stderr)
        return 1
    except CommandError as error:
        print(str(error), file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f"command {error.cmd!r} exited with status {error.returncode}", file=sys.stderr)
        return 1

    json.dump(response, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0
