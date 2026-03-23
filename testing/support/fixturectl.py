#!/usr/bin/env python3

import argparse
import json
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURE_ROOT = REPO_ROOT / "testing" / "ssh-fixture"
DEFAULT_IMAGE = "track-testing/ssh-fixture:local"
DEFAULT_RUNTIME_DIR = Path("/tmp/track-testing-runtime")
FIXTURE_RUNTIME_MOUNT = Path("/srv/track-testing")


def run(command: list[str], *, capture_output: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        check=True,
        cwd=REPO_ROOT,
        text=True,
        capture_output=capture_output,
    )


def ensure_runtime_layout(runtime_dir: Path) -> None:
    for relative_path in [
        "state",
        "logs",
        "git",
    ]:
        (runtime_dir / relative_path).mkdir(parents=True, exist_ok=True)


def load_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default

    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")


def repo_name_from_url(repo_url: str) -> str:
    return repo_url.rstrip("/").removesuffix(".git").split("/")[-1]


def runtime_path_inside_fixture(runtime_dir: Path, host_path: Path) -> Path:
    return FIXTURE_RUNTIME_MOUNT / host_path.resolve().relative_to(runtime_dir.resolve())


def command_build(args: argparse.Namespace) -> None:
    run(
        [
            "docker",
            "build",
            "-t",
            args.image,
            "-f",
            str(FIXTURE_ROOT / "Dockerfile"),
            str(FIXTURE_ROOT),
        ]
    )
    print(json.dumps({"image": args.image}))


def command_run(args: argparse.Namespace) -> None:
    runtime_dir = args.runtime_dir.resolve()
    ensure_runtime_layout(runtime_dir)

    if args.authorized_key is not None:
        shutil.copyfile(args.authorized_key, runtime_dir / "authorized_keys")

    run(
        [
            "docker",
            "run",
            "--detach",
            "--rm",
            "--name",
            args.name,
            "--publish",
            f"{args.port}:22",
            "--volume",
            f"{runtime_dir}:/srv/track-testing",
            args.image,
        ]
    )

    print(
        json.dumps(
            {
                "container": args.name,
                "host": "127.0.0.1",
                "image": args.image,
                "port": args.port,
                "runtimeDir": str(runtime_dir),
            }
        )
    )


def command_stop(args: argparse.Namespace) -> None:
    run(["docker", "rm", "--force", args.name])
    print(json.dumps({"container": args.name, "stopped": True}))


def command_write_state(args: argparse.Namespace) -> None:
    runtime_dir = args.runtime_dir.resolve()
    ensure_runtime_layout(runtime_dir)

    with args.source.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)

    destination = runtime_dir / "state" / f"{args.target}.json"
    write_json(destination, payload)
    print(json.dumps({"path": str(destination), "target": args.target}))


def command_generate_key(args: argparse.Namespace) -> None:
    destination_prefix = args.output_prefix.resolve()
    destination_prefix.parent.mkdir(parents=True, exist_ok=True)

    for suffix in ["", ".pub"]:
        candidate = Path(f"{destination_prefix}{suffix}")
        if candidate.exists():
            candidate.unlink()

    run(
        [
            "ssh-keygen",
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-f",
            str(destination_prefix),
        ]
    )
    print(
        json.dumps(
            {
                "privateKey": str(destination_prefix),
                "publicKey": str(destination_prefix.with_name(destination_prefix.name + ".pub")),
            }
        )
    )


def command_wait_for_ssh(args: argparse.Namespace) -> None:
    deadline = time.monotonic() + args.timeout_seconds
    last_error: str | None = None
    known_hosts_path = args.known_hosts.resolve()
    known_hosts_path.parent.mkdir(parents=True, exist_ok=True)
    known_hosts_path.touch(exist_ok=True)

    while time.monotonic() < deadline:
        try:
            completed = subprocess.run(
                [
                    "ssh",
                    "-i",
                    str(args.private_key.resolve()),
                    "-p",
                    str(args.port),
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "IdentitiesOnly=yes",
                    "-o",
                    "StrictHostKeyChecking=accept-new",
                    "-o",
                    f"UserKnownHostsFile={known_hosts_path}",
                    f"{args.user}@{args.host}",
                    "true",
                ],
                check=True,
                text=True,
                capture_output=True,
            )
            print(
                json.dumps(
                    {
                        "host": args.host,
                        "knownHosts": str(known_hosts_path),
                        "port": args.port,
                        "ready": True,
                        "user": args.user,
                    }
                )
            )
            _ = completed
            return
        except subprocess.CalledProcessError as error:
            last_error = (error.stderr or error.stdout or str(error)).strip()
            time.sleep(0.5)

    raise SystemExit(
        json.dumps(
            {
                "host": args.host,
                "knownHosts": str(known_hosts_path),
                "port": args.port,
                "ready": False,
                "reason": last_error or "timed out waiting for SSH",
                "user": args.user,
            }
        )
    )


def command_seed_repo(args: argparse.Namespace) -> None:
    runtime_dir = args.runtime_dir.resolve()
    ensure_runtime_layout(runtime_dir)

    repo_name = args.repo_name or repo_name_from_url(args.repo_url)
    upstream_bare_path = runtime_dir / "git" / "upstream" / f"{repo_name}.git"
    fork_bare_path = runtime_dir / "git" / args.fork_owner / f"{repo_name}.git"
    upstream_bare_path_in_fixture = runtime_path_inside_fixture(runtime_dir, upstream_bare_path)
    fork_bare_path_in_fixture = runtime_path_inside_fixture(runtime_dir, fork_bare_path)

    if not upstream_bare_path.exists():
        with tempfile.TemporaryDirectory(prefix="track-testing-repo-") as temporary_directory:
            working_copy = Path(temporary_directory) / repo_name
            run(["git", "init", "-b", args.base_branch, str(working_copy)])
            run(["git", "-C", str(working_copy), "config", "user.name", "Track Testing"])
            run(
                [
                    "git",
                    "-C",
                    str(working_copy),
                    "config",
                    "user.email",
                    "track-testing@example.com",
                ]
            )

            (working_copy / "README.md").write_text(
                f"# {repo_name}\n\nSeed repository for track integration tests.\n",
                encoding="utf-8",
            )
            run(["git", "-C", str(working_copy), "add", "README.md"])
            run(["git", "-C", str(working_copy), "commit", "-m", "chore: seed fixture repository"])

            upstream_bare_path.parent.mkdir(parents=True, exist_ok=True)
            run(["git", "clone", "--bare", str(working_copy), str(upstream_bare_path)])

    gh_state_path = runtime_dir / "state" / "gh.json"
    gh_state = load_json(gh_state_path, {"login": args.fork_owner, "repositories": {}})
    gh_state["login"] = args.login or gh_state.get("login") or args.fork_owner
    gh_state.setdefault("repositories", {})
    gh_state["repositories"][args.repo_url] = {
        "name": repo_name,
        "upstreamBarePath": str(upstream_bare_path_in_fixture),
        "forkOwner": args.fork_owner,
        "forkBarePath": str(fork_bare_path_in_fixture),
    }
    write_json(gh_state_path, gh_state)

    print(
        json.dumps(
            {
                "forkBarePath": str(fork_bare_path),
                "forkBarePathInFixture": str(fork_bare_path_in_fixture),
                "ghStatePath": str(gh_state_path),
                "repoName": repo_name,
                "repoUrl": args.repo_url,
                "upstreamBarePath": str(upstream_bare_path),
                "upstreamBarePathInFixture": str(upstream_bare_path_in_fixture),
            }
        )
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Manage the track SSH testing fixture.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    build_parser = subparsers.add_parser("build-image", help="Build the SSH fixture Docker image.")
    build_parser.add_argument("--image", default=DEFAULT_IMAGE)
    build_parser.set_defaults(func=command_build)

    run_parser = subparsers.add_parser("run", help="Run the SSH fixture container.")
    run_parser.add_argument("--image", default=DEFAULT_IMAGE)
    run_parser.add_argument("--name", required=True)
    run_parser.add_argument("--port", type=int, required=True)
    run_parser.add_argument("--runtime-dir", type=Path, default=DEFAULT_RUNTIME_DIR)
    run_parser.add_argument("--authorized-key", type=Path)
    run_parser.set_defaults(func=command_run)

    stop_parser = subparsers.add_parser("stop", help="Stop the SSH fixture container.")
    stop_parser.add_argument("--name", required=True)
    stop_parser.set_defaults(func=command_stop)

    write_state_parser = subparsers.add_parser(
        "write-state",
        help="Write a mock state JSON file into the fixture runtime directory.",
    )
    write_state_parser.add_argument("--runtime-dir", type=Path, default=DEFAULT_RUNTIME_DIR)
    write_state_parser.add_argument("--target", choices=["gh", "codex"], required=True)
    write_state_parser.add_argument("--source", type=Path, required=True)
    write_state_parser.set_defaults(func=command_write_state)

    generate_key_parser = subparsers.add_parser(
        "generate-key",
        help="Generate an ephemeral SSH keypair for fixture access.",
    )
    generate_key_parser.add_argument("--output-prefix", type=Path, required=True)
    generate_key_parser.set_defaults(func=command_generate_key)

    wait_for_ssh_parser = subparsers.add_parser(
        "wait-for-ssh",
        help="Poll the fixture until SSH accepts the generated key.",
    )
    wait_for_ssh_parser.add_argument("--host", default="127.0.0.1")
    wait_for_ssh_parser.add_argument("--user", default="track")
    wait_for_ssh_parser.add_argument("--port", type=int, required=True)
    wait_for_ssh_parser.add_argument("--private-key", type=Path, required=True)
    wait_for_ssh_parser.add_argument("--known-hosts", type=Path, required=True)
    wait_for_ssh_parser.add_argument("--timeout-seconds", type=float, default=15.0)
    wait_for_ssh_parser.set_defaults(func=command_wait_for_ssh)

    seed_repo_parser = subparsers.add_parser(
        "seed-repo",
        help="Create a bare upstream repo and register it in gh.json.",
    )
    seed_repo_parser.add_argument("--runtime-dir", type=Path, default=DEFAULT_RUNTIME_DIR)
    seed_repo_parser.add_argument("--repo-url", required=True)
    seed_repo_parser.add_argument("--repo-name")
    seed_repo_parser.add_argument("--base-branch", default="main")
    seed_repo_parser.add_argument("--fork-owner", default="fixture-user")
    seed_repo_parser.add_argument("--login")
    seed_repo_parser.set_defaults(func=command_seed_repo)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
