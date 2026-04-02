import os
from pathlib import Path

import requests

from ..smoke_context import SmokeContext


# ==============================================================================
# Safety Guards And Harmless Connectivity Checks
# ==============================================================================
#
# The install-flow scenarios exercise the real installer and backend wrapper, so
# we reject local runs up front. The lightweight connectivity check stays here
# too because it exists to validate the Python execution path without touching
# any of the smoke's mutable infrastructure.


def ensure_ci_only_execution(context: SmokeContext) -> None:
    del context

    if os.environ.get("CI") != "1":
        raise RuntimeError(
            "This smoke script is potentially destructive and is only meant to run in CI. "
            "Refusing to continue because CI=1 is not set."
        )

    local_track_dir = Path.home() / ".track"
    if not local_track_dir.exists():
        return

    try:
        has_contents = any(local_track_dir.iterdir())
    except OSError as error:
        raise RuntimeError(
            "This smoke script is potentially destructive and is only meant to run in CI. "
            f"Refusing to inspect {local_track_dir}: {error}"
        ) from error

    if has_contents:
        raise RuntimeError(
            "This smoke script is potentially destructive and is only meant to run in CI. "
            f"Refusing to continue because {local_track_dir} is not empty."
        )


def request_example_dot_com(context: SmokeContext) -> None:
    del context

    # This scenario exists so we can validate the venv and third-party
    # dependency path locally without starting Docker, touching track state, or
    # invoking any installer-managed side effects. We intentionally use plain
    # HTTP here so the check does not depend on the local certificate store.
    response = requests.get("http://example.com", timeout=5)
    response.raise_for_status()
    if "Example Domain" not in response.text:
        raise RuntimeError("The example.com probe did not return the expected marker text.")


def print_check_successful(context: SmokeContext) -> None:
    del context
    print("Check successful")
