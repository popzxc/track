import hashlib
import json
import os
import fcntl
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


RUNTIME_DIR = Path(os.environ.get("TRACK_TESTING_RUNTIME_DIR", "/srv/track-testing"))


def utc_timestamp() -> str:
    return datetime.now(tz=timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def load_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default

    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Any) -> None:
    ensure_parent(path)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")


def append_jsonl(path: Path, payload: dict[str, Any]) -> None:
    ensure_parent(path)
    with path.open("a", encoding="utf-8") as handle:
        json.dump(payload, handle, sort_keys=True)
        handle.write("\n")


def lock_path(name: str) -> Path:
    digest = hashlib.sha256(name.encode("utf-8")).hexdigest()
    return RUNTIME_DIR / "locks" / f"{digest}.lock"


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


def state_path(name: str) -> Path:
    return RUNTIME_DIR / "state" / f"{name}.json"


def log_path(name: str) -> Path:
    return RUNTIME_DIR / "logs" / f"{name}.jsonl"
