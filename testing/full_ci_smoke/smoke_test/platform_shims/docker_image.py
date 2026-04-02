import os
from pathlib import Path
import json

from .docker_state import load_docker_metadata, resolve_cargo_target_dir, save_docker_metadata
from .utils import run_subprocess


# ==============================================================================
# Strict Docker Image Subcommands
# ==============================================================================
#
# The smoke only needs a tiny subset of top-level Docker behavior to mimic what
# `trackup` and the installed backend wrapper do in CI: build the image and
# confirm that it exists. We keep those commands separate from the Compose
# lifecycle so each contract stays small and explicit.


def docker_image_main(argv: list[str]) -> int:
    if argv[:1] == ["build"]:
        return docker_build(argv)
    if argv[:2] == ["image", "inspect"]:
        return docker_image_inspect(argv)

    raise SystemExit(f"Unsupported docker image invocation: {argv}")


def docker_build(argv: list[str]) -> int:
    if len(argv) != 6 or argv[0] != "build" or argv[1] != "-t" or argv[3] != "--build-arg":
        raise SystemExit(f"Unsupported docker build invocation: {argv}")

    image_tag = argv[2]
    build_arg = argv[4]
    if not build_arg.startswith("TRACK_GIT_COMMIT="):
        raise SystemExit(f"Unsupported docker build arg: {build_arg}")

    git_commit = build_arg.partition("=")[2]
    context_path = Path(argv[5]).resolve()
    frontend_dir = context_path / "frontend"
    if not frontend_dir.is_dir():
        raise SystemExit(f"Expected frontend directory under build context {context_path}.")

    run_subprocess(["bun", "install", "--frozen-lockfile"], cwd=frontend_dir)
    run_subprocess(["bun", "run", "build"], cwd=frontend_dir)

    build_env = os.environ.copy()
    build_env["TRACK_GIT_COMMIT"] = git_commit
    run_subprocess(
        ["cargo", "build", "--release", "-p", "track-api"],
        cwd=context_path,
        env=build_env,
    )

    metadata = load_docker_metadata()
    metadata.setdefault("images", {})[image_tag] = {
        "context": str(context_path),
        "gitCommit": git_commit,
        "staticRoot": str(context_path / "frontend" / "dist"),
        "trackApiPath": str(resolve_cargo_target_dir(context_path) / "release" / "track-api"),
    }
    save_docker_metadata(metadata)
    return 0


def docker_image_inspect(argv: list[str]) -> int:
    if len(argv) != 3 or argv[:2] != ["image", "inspect"]:
        raise SystemExit(f"Unsupported docker image inspect invocation: {argv}")

    image_tag = argv[2]
    metadata = load_docker_metadata()
    image = metadata.get("images", {}).get(image_tag)
    if image is None:
        raise SystemExit(f"Mock docker image {image_tag!r} is not available.")

    print(
        json.dumps(
            [
                {
                    "Config": {"Labels": {"track.gitCommit": image["gitCommit"]}},
                    "RepoTags": [image_tag],
                }
            ]
        )
    )
    return 0
