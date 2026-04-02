import os
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from .constants import (
    FIXTURE_SHELL_PRELUDE,
    LINUX_FIXTURE_REMOTE_HOST,
    LINUX_FIXTURE_REMOTE_PORT,
    LINUX_FIXTURE_REMOTE_USER,
    MACOS_HOST_FIXTURE_REMOTE_HOST,
    MACOS_HOST_FIXTURE_REMOTE_USER,
    PROJECT_NAME,
    TASK_TITLE,
)

SmokePlatform = Literal["linux-docker", "macos-host"]


@dataclass(frozen=True)
class InstallFlowOptions:
    platform: SmokePlatform
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
    shim_bin_dir: Path
    shim_state_dir: Path
    remote_home_dir: Path
    remote_bin_dir: Path
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
        env = self.host_tool_env()
        env["HOME"] = str(self.home_dir)
        if extra is not None:
            env.update(extra)
        return env

    def host_tool_env(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        env = os.environ.copy()
        if self.install_flow_options is not None and self.install_flow_options.platform == "macos-host":
            env["PATH"] = f"{self.shim_bin_dir}:{env.get('PATH', '')}"
            env["TRACK_SMOKE_DOCKER_STATE_DIR"] = str(self.shim_state_dir / "docker")
            env["TRACK_SMOKE_REMOTE_HOME"] = str(self.remote_home_dir)
            env["TRACK_SMOKE_REMOTE_RUNTIME_DIR"] = str(self.fixture_runtime_dir)
            env["TRACK_SMOKE_REMOTE_BIN_DIR"] = str(self.remote_bin_dir)
            env["TRACK_SMOKE_EXPECTED_REMOTE_HOST"] = self.remote_agent_host()
            env["TRACK_SMOKE_EXPECTED_REMOTE_PORT"] = str(self.remote_agent_port())
            env["TRACK_SMOKE_EXPECTED_REMOTE_USER"] = self.remote_agent_user()
        if extra is not None:
            env.update(extra)
        return env

    def backend_env(self) -> dict[str, str]:
        if self.backend_port is None:
            raise RuntimeError("The smoke scenario did not reserve a backend port.")
        return self.smoke_env({"TRACK_WEB_PORT": str(self.backend_port)})

    def fixture_shell_prelude(self) -> str:
        if self.install_flow_options is None:
            raise RuntimeError("The install-flow scenario did not initialize its options.")

        if self.install_flow_options.platform == "linux-docker":
            return FIXTURE_SHELL_PRELUDE

        # The host-mode fixture runs the remote scripts locally through strict
        # SSH/SCP shims, so the remote PATH and runtime dir need to point at the
        # temp directories we created for this scenario rather than the Linux
        # container locations used by the Docker fixture.
        return (
            f'export PATH="{self.remote_bin_dir}:$PATH"\n'
            f'export TRACK_TESTING_RUNTIME_DIR="{self.fixture_runtime_dir}"'
        )

    def remote_agent_host(self) -> str:
        if self.install_flow_options is None:
            raise RuntimeError("The install-flow scenario did not initialize its options.")

        if self.install_flow_options.platform == "linux-docker":
            return LINUX_FIXTURE_REMOTE_HOST

        return MACOS_HOST_FIXTURE_REMOTE_HOST

    def remote_agent_port(self) -> int:
        if self.install_flow_options is None:
            raise RuntimeError("The install-flow scenario did not initialize its options.")

        if self.install_flow_options.platform == "linux-docker":
            return LINUX_FIXTURE_REMOTE_PORT

        if self.fixture_port is None:
            raise RuntimeError("The install-flow scenario did not reserve a fixture port.")
        return self.fixture_port

    def remote_agent_user(self) -> str:
        if self.install_flow_options is None:
            raise RuntimeError("The install-flow scenario did not initialize its options.")

        if self.install_flow_options.platform == "linux-docker":
            return LINUX_FIXTURE_REMOTE_USER

        return MACOS_HOST_FIXTURE_REMOTE_USER


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
    shim_bin_dir = temp_root / "host-shims" / "bin"
    shim_state_dir = temp_root / "host-shims" / "state"
    remote_home_dir = temp_root / "remote-home"
    remote_bin_dir = temp_root / "remote-bin"

    return SmokeContext(
        revision=revision,
        expected_commit=expected_commit,
        temp_root=temp_root,
        home_dir=home_dir,
        fixture_runtime_dir=fixture_runtime_dir,
        fixture_key_prefix=fixture_key_prefix,
        project_checkout=project_checkout,
        installed_bin_dir=installed_bin_dir,
        shim_bin_dir=shim_bin_dir,
        shim_state_dir=shim_state_dir,
        remote_home_dir=remote_home_dir,
        remote_bin_dir=remote_bin_dir,
        track_backend_path=installed_bin_dir / "track-backend",
        track_cli_path=installed_bin_dir / "track",
        fixture_port=None,
        backend_port=None,
        api_base_url=None,
    )
