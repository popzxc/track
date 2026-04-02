import os
import tempfile
from dataclasses import dataclass
from pathlib import Path

from .constants import PROJECT_NAME, TASK_TITLE


@dataclass(frozen=True)
class InstallFlowOptions:
    use_default_backend_port: bool
    configure_cli_with_backend_url: bool
    remote_agent_workspace_root: str | None
    remote_agent_projects_registry_path: str | None


@dataclass
class SmokeContext:
    revision: str | None
    expected_commit: str | None
    temp_root: Path
    home_dir: Path
    fixture_runtime_dir: Path
    fixture_key_prefix: Path
    project_checkout: Path
    installed_bin_dir: Path
    track_backend_path: Path
    track_cli_path: Path
    fixture_port: int | None
    backend_port: int | None
    api_base_url: str | None
    task_title: str = TASK_TITLE
    short_revision: str | None = None
    image_tag: str | None = None
    fixture_container_name: str | None = None
    fixture_running: bool = False
    backend_running: bool = False
    task_id: str | None = None
    dispatch_id: str | None = None
    review_id: str | None = None
    pull_request_url: str | None = None
    install_flow_options: InstallFlowOptions | None = None
    source_checkout_dir: Path | None = None
    resolved_commit: str | None = None

    def smoke_env(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        env = os.environ.copy()
        env["HOME"] = str(self.home_dir)
        if extra is not None:
            env.update(extra)
        return env

    def backend_env(self) -> dict[str, str]:
        if self.backend_port is None:
            raise RuntimeError("The smoke scenario did not reserve a backend port.")
        return self.smoke_env({"TRACK_WEB_PORT": str(self.backend_port)})


def create_context(revision: str | None, expected_commit: str | None) -> SmokeContext:
    # We isolate the smoke inside a throwaway HOME so the installed scripts,
    # generated config, and Docker compose artifacts never point at the caller's
    # real track state.
    temp_root = Path(tempfile.mkdtemp(prefix="track-install-smoke-"))
    home_dir = temp_root / "home"
    home_dir.mkdir(parents=True, exist_ok=True)

    fixture_runtime_dir = temp_root / "fixture-runtime"
    fixture_key_prefix = temp_root / "fixture-key" / "id_ed25519"
    project_checkout = temp_root / "workspace" / PROJECT_NAME
    installed_bin_dir = home_dir / ".track" / "bin"

    return SmokeContext(
        revision=revision,
        expected_commit=expected_commit,
        temp_root=temp_root,
        home_dir=home_dir,
        fixture_runtime_dir=fixture_runtime_dir,
        fixture_key_prefix=fixture_key_prefix,
        project_checkout=project_checkout,
        installed_bin_dir=installed_bin_dir,
        track_backend_path=installed_bin_dir / "track-backend",
        track_cli_path=installed_bin_dir / "track",
        fixture_port=None,
        backend_port=None,
        api_base_url=None,
    )
