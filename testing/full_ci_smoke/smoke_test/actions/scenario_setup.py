import json
from pathlib import Path

from ..constants import (
    FIXTURECTL_PATH,
    FIXTURE_GH_LOGIN,
    FIXTURE_IMAGE,
    LINUX_FIXTURE_NETWORK,
    LINUX_FIXTURE_NETWORK_ALIAS,
    LINUX_FIXTURE_PROBE_HOST,
    LINUX_FIXTURE_PROJECTS_REGISTRY_PATH,
    LINUX_FIXTURE_REMOTE_USER,
    LINUX_FIXTURE_WORKSPACE_ROOT,
    MACOS_HOST_FIXTURE_PROJECTS_REGISTRY_PATH,
    MACOS_HOST_FIXTURE_WORKSPACE_ROOT,
    PROJECT_REPO_URL,
    REPO_ROOT,
)
from ..platform_setup import ensure_host_fixture_layout, install_macos_host_shims
from ..shell_utils import reserve_local_port, run_json, write_text
from ..smoke_context import InstallFlowOptions, SmokeContext


# ==============================================================================
# Scenario Configuration And Platform Fixture Setup
# ==============================================================================
#
# The scenario layer decides which platform contract it wants to exercise. This
# module turns that declarative choice into concrete runtime ports, strict host
# shims, and either the Docker-backed Linux fixture or the host-backed macOS
# fixture state.


def apply_install_flow_linux_docker_defaults(context: SmokeContext) -> None:
    context.install_flow_options = InstallFlowOptions(
        platform="linux-docker",
        use_default_backend_port=True,
        configure_cli_with_backend_url=False,
        remote_agent_workspace_root=None,
        remote_agent_projects_registry_path=None,
    )


def apply_install_flow_linux_docker_overrides(context: SmokeContext) -> None:
    context.install_flow_options = InstallFlowOptions(
        platform="linux-docker",
        use_default_backend_port=False,
        configure_cli_with_backend_url=True,
        remote_agent_workspace_root=LINUX_FIXTURE_WORKSPACE_ROOT,
        remote_agent_projects_registry_path=LINUX_FIXTURE_PROJECTS_REGISTRY_PATH,
    )


def apply_install_flow_macos_host_defaults(context: SmokeContext) -> None:
    context.install_flow_options = InstallFlowOptions(
        platform="macos-host",
        use_default_backend_port=True,
        configure_cli_with_backend_url=False,
        remote_agent_workspace_root=None,
        remote_agent_projects_registry_path=None,
    )


def apply_install_flow_macos_host_overrides(context: SmokeContext) -> None:
    context.install_flow_options = InstallFlowOptions(
        platform="macos-host",
        use_default_backend_port=False,
        configure_cli_with_backend_url=True,
        remote_agent_workspace_root=MACOS_HOST_FIXTURE_WORKSPACE_ROOT,
        remote_agent_projects_registry_path=MACOS_HOST_FIXTURE_PROJECTS_REGISTRY_PATH,
    )


def prepare_install_flow_runtime(context: SmokeContext) -> None:
    if context.install_flow_options is None:
        raise RuntimeError("The install-flow scenario did not initialize its options.")

    # Both install scenarios use a dynamic SSH fixture port because that bind is
    # environment-sensitive. The backend host port is the interesting default we
    # can safely exercise in CI.
    context.fixture_port = reserve_local_port()
    if context.install_flow_options.use_default_backend_port:
        context.backend_port = 3210
    else:
        context.backend_port = reserve_local_port()
    context.api_base_url = f"http://127.0.0.1:{context.backend_port}"


def prepare_macos_host_tooling(context: SmokeContext) -> None:
    if context.install_flow_options is None:
        raise RuntimeError("The install-flow scenario did not initialize its options.")
    if context.install_flow_options.platform != "macos-host":
        raise RuntimeError("macOS host tooling is only valid for the macOS host scenario.")

    install_macos_host_shims(context)


def start_linux_docker_fixture(context: SmokeContext) -> None:
    if context.fixture_port is None:
        raise RuntimeError("The install-flow scenario did not reserve a fixture port.")
    if context.fixture_container_name is None:
        raise RuntimeError("Fixture container name is not prepared.")

    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "build-image",
            "--image",
            FIXTURE_IMAGE,
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "generate-key",
            "--output-prefix",
            str(context.fixture_key_prefix),
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "run",
            "--image",
            FIXTURE_IMAGE,
            "--name",
            context.fixture_container_name,
            "--port",
            str(context.fixture_port),
            "--runtime-dir",
            str(context.fixture_runtime_dir),
            "--authorized-key",
            f"{context.fixture_key_prefix}.pub",
            "--network",
            LINUX_FIXTURE_NETWORK,
            "--network-alias",
            LINUX_FIXTURE_NETWORK_ALIAS,
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    context.fixture_running = True

    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "wait-for-ssh",
            "--host",
            LINUX_FIXTURE_PROBE_HOST,
            "--user",
            LINUX_FIXTURE_REMOTE_USER,
            "--port",
            str(context.fixture_port),
            "--private-key",
            str(context.fixture_key_prefix),
            "--known-hosts",
            str(context.fixture_runtime_dir / "known_hosts"),
            "--timeout-seconds",
            "20",
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    seed_fixture_repository(context)
    write_successful_codex_state(context)


def start_macos_host_fixture(context: SmokeContext) -> None:
    if context.fixture_port is None:
        raise RuntimeError("The install-flow scenario did not reserve a fixture port.")

    ensure_host_fixture_layout(context)
    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "generate-key",
            "--output-prefix",
            str(context.fixture_key_prefix),
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    write_text(context.fixture_runtime_dir / "known_hosts", "")
    seed_fixture_repository(context)
    rewrite_host_mode_gh_state_paths(context)
    context.fixture_running = True
    write_successful_codex_state(context)


def seed_fixture_repository(context: SmokeContext) -> None:
    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "seed-repo",
            "--runtime-dir",
            str(context.fixture_runtime_dir),
            "--repo-url",
            PROJECT_REPO_URL,
            "--base-branch",
            "main",
            "--fork-owner",
            FIXTURE_GH_LOGIN,
            "--login",
            FIXTURE_GH_LOGIN,
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )


def write_successful_codex_state(context: SmokeContext) -> None:
    write_text(
        context.fixture_runtime_dir / "state" / "codex.json",
        json.dumps(
            {
                "mode": "success",
                "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
                "committed": True,
                "summary": "Install smoke task completed through the remote fixture.",
                "reviewSubmitted": True,
                "githubReviewId": "42001",
                "githubReviewUrl": "https://github.com/acme/project-a/pull/42#pullrequestreview-42001",
                "reviewBody": "@octocat requested me to review this PR.\n\nInstall smoke review completed successfully.",
            },
            indent=2,
        )
        + "\n",
    )


def rewrite_host_mode_gh_state_paths(context: SmokeContext) -> None:
    gh_state_path = context.fixture_runtime_dir / "state" / "gh.json"
    gh_state = json.loads(gh_state_path.read_text(encoding="utf-8"))

    # `fixturectl seed-repo` writes repository paths the way the Docker fixture
    # sees them from inside the container mount, for example
    # `/srv/track-testing/git/...`. The macOS host-mode smoke runs the gh mock
    # directly on the runner, so those same fields must point at real host paths
    # under the temp runtime directory instead of the container-only mount path.
    for repository in gh_state.get("repositories", {}).values():
        for field_name in ["upstreamBarePath", "forkBarePath"]:
            field_value = repository.get(field_name)
            if not isinstance(field_value, str) or not field_value.startswith("/srv/track-testing/"):
                continue

            relative_path = Path(field_value).relative_to("/srv/track-testing")
            repository[field_name] = str(context.fixture_runtime_dir / relative_path)

    write_text(gh_state_path, json.dumps(gh_state, indent=2) + "\n")
