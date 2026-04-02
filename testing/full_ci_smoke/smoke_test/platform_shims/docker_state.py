import json
import os
from pathlib import Path


def load_docker_metadata() -> dict:
    path = docker_metadata_file()
    if not path.is_file():
        return {"images": {}}
    return json.loads(path.read_text(encoding="utf-8"))


def save_docker_metadata(payload: dict) -> None:
    path = docker_metadata_file()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def docker_process_state() -> dict | None:
    path = docker_process_file()
    if not path.is_file():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def save_docker_process_state(payload: dict) -> None:
    path = docker_process_file()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def docker_metadata_file() -> Path:
    return docker_state_root() / "metadata.json"


def docker_process_file() -> Path:
    return docker_state_root() / "process.json"


def docker_state_root() -> Path:
    value = os.environ.get("TRACK_SMOKE_DOCKER_STATE_DIR")
    if not value:
        raise SystemExit("Mock docker requires TRACK_SMOKE_DOCKER_STATE_DIR.")
    return Path(value)


def resolve_cargo_target_dir(context_path: Path) -> Path:
    configured_target_dir = os.environ.get("CARGO_TARGET_DIR")
    if not configured_target_dir:
        return context_path / "target"

    target_dir = Path(configured_target_dir)
    if target_dir.is_absolute():
        return target_dir
    return context_path / target_dir
