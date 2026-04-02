import logging
import os
from pathlib import Path


_FORMAT = "%(asctime)s %(levelname)s %(name)s: %(message)s"


def configure_component_logger(
    component: str,
    *,
    log_dir: Path | None = None,
    mirror_to_stderr: bool = False,
) -> logging.Logger:
    logger = logging.getLogger(f"track_smoke.{component}")
    if getattr(logger, "_track_smoke_configured", False):
        return logger

    logger.setLevel(log_level())
    logger.propagate = False

    target_log_dir = log_dir or env_log_dir()
    if target_log_dir is not None:
        target_log_dir.mkdir(parents=True, exist_ok=True)
        file_handler = logging.FileHandler(
            target_log_dir / f"{sanitize_component_name(component)}.log",
            encoding="utf-8",
        )
        file_handler.setFormatter(logging.Formatter(_FORMAT))
        logger.addHandler(file_handler)

    if mirror_to_stderr:
        stream_handler = logging.StreamHandler()
        stream_handler.setFormatter(logging.Formatter(_FORMAT))
        logger.addHandler(stream_handler)

    logger._track_smoke_configured = True  # type: ignore[attr-defined]
    return logger


def env_log_dir() -> Path | None:
    value = os.environ.get("TRACK_SMOKE_LOG_DIR")
    if not value:
        return None
    return Path(value)


def log_level() -> int:
    configured = os.environ.get("TRACK_SMOKE_LOG_LEVEL", "INFO").upper()
    return getattr(logging, configured, logging.INFO)


def sanitize_component_name(component: str) -> str:
    return component.replace(".", "_")
