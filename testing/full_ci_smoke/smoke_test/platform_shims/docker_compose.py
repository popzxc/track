import os
from pathlib import Path
import signal
import subprocess
import sys
import time

from ..logging_utils import configure_component_logger
from .docker_state import (
    docker_process_file,
    docker_process_state,
    docker_state_root,
    load_docker_metadata,
    save_docker_process_state,
)
from .utils import process_is_alive


# ==============================================================================
# Strict Docker Compose Subcommands
# ==============================================================================
#
# The installed `track-backend` wrapper always talks to Docker through a fixed
# Compose command shape. This host-mode shim mirrors only those exact
# subcommands so any drift in the wrapper contract breaks loudly in CI.


logger = configure_component_logger("platform_shims.docker_compose")


def docker_compose_main(argv: list[str]) -> int:
    if argv == ["compose", "version"]:
        print("Docker Compose version v2.99.0-smoke")
        return 0
    if argv[:1] == ["compose"]:
        return docker_compose(argv)

    raise SystemExit(f"Unsupported docker compose invocation: {argv}")


def docker_compose(argv: list[str]) -> int:
    if len(argv) < 6:
        raise SystemExit(f"Unsupported docker compose invocation: {argv}")
    if argv[0] != "compose" or argv[1] != "--project-name" or argv[2] != "track":
        raise SystemExit(f"Unsupported docker compose invocation: {argv}")
    if argv[3] != "-f":
        raise SystemExit(f"Unsupported docker compose invocation: {argv}")

    compose_path = Path(argv[4]).resolve()
    subcommand = argv[5]
    remaining = argv[6:]

    if subcommand == "up":
        if remaining != ["-d"]:
            raise SystemExit(f"Unsupported docker compose up invocation: {argv}")
        return docker_compose_up(compose_path)
    if subcommand == "down":
        if remaining != ["-v", "--remove-orphans"]:
            raise SystemExit(f"Unsupported docker compose down invocation: {argv}")
        return docker_compose_down()
    if subcommand == "ps":
        if remaining:
            raise SystemExit(f"Unsupported docker compose ps invocation: {argv}")
        return docker_compose_ps()
    if subcommand == "logs":
        if remaining != ["--no-color"]:
            raise SystemExit(f"Unsupported docker compose logs invocation: {argv}")
        return docker_compose_logs()

    raise SystemExit(f"Unsupported docker compose invocation: {argv}")


def docker_compose_up(compose_path: Path) -> int:
    plan = compose_plan_from_file(compose_path)
    metadata = load_docker_metadata()
    image = metadata.get("images", {}).get(plan["imageTag"])
    if image is None:
        raise SystemExit(f"Mock docker image {plan['imageTag']!r} is not available.")

    track_api_path = Path(image["trackApiPath"])
    if not track_api_path.is_file():
        raise SystemExit(f"Expected built track-api binary at {track_api_path}.")

    current_process = docker_process_state()
    if current_process is not None and process_is_alive(int(current_process["pid"])):
        raise SystemExit("Mock docker compose already has a running track-web process.")
    if current_process is not None and not process_is_alive(int(current_process["pid"])):
        docker_process_file().unlink(missing_ok=True)

    log_path = docker_state_root() / "backend.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log_handle = log_path.open("a", encoding="utf-8")

    home_value = os.environ.get("HOME")
    if not home_value:
        raise SystemExit("Mock docker compose requires HOME to be set.")
    home_dir = Path(home_value).resolve()
    (home_dir / ".track" / "backend").mkdir(parents=True, exist_ok=True)
    (home_dir / ".config" / "track").mkdir(parents=True, exist_ok=True)

    api_env = os.environ.copy()
    api_env.update(
        {
            "HOME": str(home_dir),
            "PORT": str(plan["hostPort"]),
            "TRACK_BIND_HOST": plan["bindHost"],
            "TRACK_STATE_DIR": str(home_dir / ".track" / "backend"),
            "TRACK_LEGACY_ROOT": str(home_dir / ".track"),
            "TRACK_LEGACY_CONFIG_PATH": str(home_dir / ".config" / "track" / "config.json"),
            "TRACK_STATIC_ROOT": image["staticRoot"],
        }
    )
    logger.debug(
        "docker_compose_up compose_path=%s image=%s bind_host=%s host_port=%s track_state_dir=%s",
        compose_path,
        plan["imageTag"],
        plan["bindHost"],
        plan["hostPort"],
        home_dir / ".track" / "backend",
    )

    child = subprocess.Popen(
        [str(track_api_path)],
        cwd=image["context"],
        env=api_env,
        stdout=log_handle,
        stderr=subprocess.STDOUT,
        text=True,
    )
    log_handle.close()
    time.sleep(0.5)
    logger.debug("docker_compose_up spawned pid=%s", child.pid)

    save_docker_process_state(
        {
            "bindHost": plan["bindHost"],
            "composePath": str(compose_path),
            "hostPort": plan["hostPort"],
            "imageTag": plan["imageTag"],
            "logPath": str(log_path),
            "pid": child.pid,
        }
    )
    return 0


def docker_compose_down() -> int:
    process_state = docker_process_state()
    if process_state is None:
        return 0

    pid = int(process_state["pid"])
    if process_is_alive(pid):
        os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not process_is_alive(pid):
                break
            time.sleep(0.2)
        if process_is_alive(pid):
            os.kill(pid, signal.SIGKILL)

    docker_process_file().unlink(missing_ok=True)
    logger.debug("docker_compose_down cleaned up process state")
    return 0


def docker_compose_ps() -> int:
    process_state = docker_process_state()
    if process_state is None or not process_is_alive(int(process_state["pid"])):
        print("NAME\tIMAGE\tSTATUS\tPORTS")
        return 0

    print("NAME\tIMAGE\tSTATUS\tPORTS")
    print(
        "track-web-1\t{image}\trunning\t{host}:{port}->3210/tcp".format(
            image=process_state["imageTag"],
            host=process_state["bindHost"],
            port=process_state["hostPort"],
        )
    )
    return 0


def docker_compose_logs() -> int:
    process_state = docker_process_state()
    if process_state is None:
        return 0

    log_path = Path(process_state["logPath"])
    if log_path.is_file():
        sys.stdout.write(log_path.read_text(encoding="utf-8"))
    return 0


def compose_plan_from_file(compose_path: Path) -> dict[str, object]:
    if not compose_path.is_file():
        raise SystemExit(f"Expected compose file at {compose_path}.")

    contents = compose_path.read_text(encoding="utf-8")
    required_snippets = [
        "services:\n  track-web:",
        'PORT: "3210"',
        "TRACK_STATE_DIR: /home/track/backend-state",
        "TRACK_LEGACY_ROOT: /home/track/legacy-home/.track",
        "TRACK_LEGACY_CONFIG_PATH: /home/track/legacy-home/.config/track/config.json",
        "source: ${HOME}/.track/backend",
        "target: /home/track/backend-state",
        "source: ${HOME}",
        "target: /home/track/legacy-home",
    ]
    for snippet in required_snippets:
        if snippet not in contents:
            raise SystemExit(
                f"Mock docker compose expected to find {snippet!r} in {compose_path}, but it was absent."
            )

    image_tag: str | None = None
    port_mapping: str | None = None
    for raw_line in contents.splitlines():
        line = raw_line.strip()
        if line.startswith("image: "):
            image_tag = line.removeprefix("image: ").strip()
        if line.startswith('- "${TRACK_WEB_BIND_HOST:-127.0.0.1}:'):
            port_mapping = line.removeprefix("- ").strip().strip('"')

    if image_tag is None:
        raise SystemExit(f"Mock docker compose could not find an image line in {compose_path}.")
    if port_mapping != "${TRACK_WEB_BIND_HOST:-127.0.0.1}:${TRACK_WEB_PORT:-3210}:3210":
        raise SystemExit(
            "Mock docker compose expected the shipped localhost-only port mapping contract."
        )

    bind_host = os.environ.get("TRACK_WEB_BIND_HOST", "127.0.0.1")
    try:
        host_port = int(os.environ.get("TRACK_WEB_PORT", "3210"))
    except ValueError as error:
        raise SystemExit(f"Mock docker compose received an invalid TRACK_WEB_PORT: {error}") from error

    return {"bindHost": bind_host, "hostPort": host_port, "imageTag": image_tag}
