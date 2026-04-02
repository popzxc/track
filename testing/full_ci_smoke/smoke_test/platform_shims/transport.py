import os
from pathlib import Path
import shutil
import subprocess
import sys


# ==============================================================================
# Strict SSH And SCP Transport Shims
# ==============================================================================
#
# The remote-agent implementation shells out through one fixed SSH/SCP shape.
# The host-mode smoke keeps that transport contract intact but maps it onto a
# local temp directory instead of a real remote machine.


def ssh_main(argv: list[str]) -> int:
    key_path, port, known_hosts_path, destination, remote_argv = parse_ssh_invocation(argv)
    validate_remote_target(key_path, port, destination)
    record_mock_known_host(known_hosts_path)

    remote_home = resolved_remote_home()
    remote_home.mkdir(parents=True, exist_ok=True)

    remote_env = os.environ.copy()
    remote_env.update(
        {
            "HOME": str(remote_home),
            "LOGNAME": expected_remote_user(),
            "SHELL": "/bin/bash",
            "USER": expected_remote_user(),
        }
    )
    normalized_remote_argv = normalize_mock_remote_home_arguments(remote_argv)
    completed = subprocess.run(
        normalized_remote_argv,
        cwd=remote_home,
        env=remote_env,
        input=sys.stdin.read(),
        text=True,
    )
    return int(completed.returncode)


def scp_main(argv: list[str]) -> int:
    key_path, port, known_hosts_path, source_path, destination = parse_scp_invocation(argv)
    validate_remote_target(key_path, port, destination_user_host(destination))
    record_mock_known_host(known_hosts_path)

    _destination_user, _, remote_path = destination.partition(":")
    if not remote_path:
        raise SystemExit(f"Unsupported scp destination: {destination}")
    if source_path == "":
        raise SystemExit("Unsupported scp invocation with an empty source path.")

    source = Path(source_path).resolve()
    if not source.is_file():
        raise SystemExit(f"Mock scp expected a local source file at {source}.")

    remote_target = expand_remote_path(remote_path)
    remote_target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, remote_target)
    return 0


def parse_ssh_invocation(argv: list[str]) -> tuple[Path, int, Path, str, list[str]]:
    if len(argv) < 15:
        raise SystemExit(f"Unsupported ssh invocation: {argv}")

    index = 0
    if argv[index] != "-i":
        raise SystemExit(f"Unsupported ssh invocation: {argv}")
    key_path = Path(argv[index + 1]).resolve()
    index += 2

    if argv[index] != "-p":
        raise SystemExit(f"Unsupported ssh invocation: {argv}")
    port = int(argv[index + 1])
    index += 2

    expected_options = [
        "BatchMode=yes",
        "IdentitiesOnly=yes",
        "ConnectTimeout=10",
        "StrictHostKeyChecking=accept-new",
    ]
    for option in expected_options:
        if argv[index:index + 2] != ["-o", option]:
            raise SystemExit(f"Unsupported ssh invocation: {argv}")
        index += 2

    if argv[index] != "-o":
        raise SystemExit(f"Unsupported ssh invocation: {argv}")
    known_hosts_option = argv[index + 1]
    if not known_hosts_option.startswith("UserKnownHostsFile="):
        raise SystemExit(f"Unsupported ssh invocation: {argv}")
    known_hosts_path = Path(known_hosts_option.partition("=")[2]).resolve()
    index += 2

    destination = argv[index]
    remote_argv = argv[index + 1 :]
    if remote_argv[:3] != ["bash", "-s", "--"]:
        raise SystemExit(f"Unsupported ssh invocation: {argv}")

    return key_path, port, known_hosts_path, destination, remote_argv


def parse_scp_invocation(argv: list[str]) -> tuple[Path, int, Path, str, str]:
    if len(argv) != 16:
        raise SystemExit(f"Unsupported scp invocation: {argv}")

    index = 0
    if argv[index] != "-i":
        raise SystemExit(f"Unsupported scp invocation: {argv}")
    key_path = Path(argv[index + 1]).resolve()
    index += 2

    if argv[index] != "-P":
        raise SystemExit(f"Unsupported scp invocation: {argv}")
    port = int(argv[index + 1])
    index += 2

    expected_options = [
        "BatchMode=yes",
        "IdentitiesOnly=yes",
        "ConnectTimeout=10",
        "StrictHostKeyChecking=accept-new",
    ]
    for option in expected_options:
        if argv[index:index + 2] != ["-o", option]:
            raise SystemExit(f"Unsupported scp invocation: {argv}")
        index += 2

    if argv[index] != "-o":
        raise SystemExit(f"Unsupported scp invocation: {argv}")
    known_hosts_option = argv[index + 1]
    if not known_hosts_option.startswith("UserKnownHostsFile="):
        raise SystemExit(f"Unsupported scp invocation: {argv}")
    known_hosts_path = Path(known_hosts_option.partition("=")[2]).resolve()
    index += 2

    return key_path, port, known_hosts_path, argv[index], argv[index + 1]


def validate_remote_target(key_path: Path, port: int, destination: str) -> None:
    if not key_path.is_file():
        raise SystemExit(f"Mock transport expected an SSH key at {key_path}.")

    if port != expected_remote_port():
        raise SystemExit(
            f"Mock transport expected remote port {expected_remote_port()}, got {port}."
        )

    if "@" not in destination:
        raise SystemExit(f"Unsupported remote target: {destination}")
    user, _, host = destination.partition("@")
    if user != expected_remote_user() or host != expected_remote_host():
        raise SystemExit(
            "Mock transport expected "
            f"{expected_remote_user()}@{expected_remote_host()}, got {destination}."
        )


def destination_user_host(destination: str) -> str:
    target, separator, _remote_path = destination.partition(":")
    if not separator:
        raise SystemExit(f"Unsupported remote target: {destination}")
    return target


def record_mock_known_host(known_hosts_path: Path) -> None:
    known_hosts_path.parent.mkdir(parents=True, exist_ok=True)
    existing_lines: list[str] = []
    if known_hosts_path.is_file():
        existing_lines = known_hosts_path.read_text(encoding="utf-8").splitlines()

    expected_entry = (
        f"{expected_remote_host()} ssh-ed25519 "
        "AAAAC3NzaC1lZDI1NTE5AAAAISMOKESMOKESMOKESMOKESMOKESMOKESMOKE"
    )
    if expected_entry not in existing_lines:
        with known_hosts_path.open("a", encoding="utf-8") as handle:
            handle.write(expected_entry + "\n")


def expected_remote_home() -> Path:
    value = os.environ.get("TRACK_SMOKE_REMOTE_HOME")
    if not value:
        raise SystemExit("Mock transport requires TRACK_SMOKE_REMOTE_HOME.")
    return Path(value)


def resolved_remote_home() -> Path:
    # GitHub's macOS runners expose the same temp directory tree through both
    # `/var/...` and `/private/var/...`. The host-mode smoke feeds the shim the
    # former, while subprocess cwd reporting and some tool outputs use the
    # latter. We normalize against both spellings and execute the mock remote
    # session from the resolved location so SSH/SCP behavior stays consistent
    # even when the runner flips between those equivalent path forms.
    return expected_remote_home().resolve()


def expected_remote_user() -> str:
    value = os.environ.get("TRACK_SMOKE_EXPECTED_REMOTE_USER")
    if not value:
        raise SystemExit("Mock transport requires TRACK_SMOKE_EXPECTED_REMOTE_USER.")
    return value


def expected_remote_host() -> str:
    value = os.environ.get("TRACK_SMOKE_EXPECTED_REMOTE_HOST")
    if not value:
        raise SystemExit("Mock transport requires TRACK_SMOKE_EXPECTED_REMOTE_HOST.")
    return value


def expected_remote_port() -> int:
    value = os.environ.get("TRACK_SMOKE_EXPECTED_REMOTE_PORT")
    if not value:
        raise SystemExit("Mock transport requires TRACK_SMOKE_EXPECTED_REMOTE_PORT.")
    return int(value)


def expand_remote_path(path_value: str) -> Path:
    path_value = normalize_mock_remote_home_path(path_value)
    remote_home = resolved_remote_home()
    if path_value == "~":
        return remote_home
    if path_value.startswith("~/"):
        return remote_home / path_value[2:]
    return Path(path_value)


def normalize_mock_remote_home_arguments(argv: list[str]) -> list[str]:
    if argv[:3] != ["bash", "-s", "--"]:
        return argv

    # The host-mode smoke mixes two transport layers: direct SCP uploads and
    # bash scripts that expect `~/...` paths to be expanded remotely. When a
    # mock-side step accidentally roots one of those shell-facing paths under
    # the fake home first, the real remote machine would still treat it as the
    # same logical path after shell expansion. Normalizing that rooted `~/`
    # shape back into the canonical remote form keeps the host-mode transport
    # behavior aligned with a real SSH session instead of inventing a sibling
    # `~/` directory under the temp home.
    normalized = argv[:3]
    normalized.extend(normalize_mock_remote_home_path(value) for value in argv[3:])
    return normalized


def normalize_mock_remote_home_path(path_value: str) -> str:
    for remote_home in remote_home_aliases():
        if path_value == f"{remote_home}/~":
            return "~"
        remote_home_prefix = f"{remote_home}/~/"
        if path_value.startswith(remote_home_prefix):
            return "~/" + path_value[len(remote_home_prefix) :]
    return path_value


def remote_home_aliases() -> list[str]:
    aliases = {str(expected_remote_home()), str(resolved_remote_home())}
    aliases.update(mac_temp_path_aliases(aliases))
    return sorted(aliases, key=len, reverse=True)


def mac_temp_path_aliases(paths: set[str]) -> set[str]:
    aliases: set[str] = set()
    for path in paths:
        if path.startswith("/var/"):
            aliases.add("/private" + path)
        if path.startswith("/private/var/"):
            aliases.add(path.removeprefix("/private"))
    return aliases
