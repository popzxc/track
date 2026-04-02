from pathlib import Path
import time

from ..api_client import TrackApiClient
from ..constants import (
    PROJECT_GIT_URL,
    PROJECT_NAME,
    REPO_ROOT,
    REVIEW_MAIN_USER,
    TRACKUP_PATH,
)
from ..shell_utils import run, wait_until, write_text
from ..smoke_context import SmokeContext


# ==============================================================================
# Installed Stack Setup
# ==============================================================================
#
# Once the fixture is ready, we switch to the exact path a user would take:
# install through trackup, start the packaged backend, and configure the
# installed CLI against that live backend.


def prepare_source_checkout(context: SmokeContext) -> None:
    if context.source_checkout_dir is not None:
        return

    source_revision = context.expected_commit or context.revision
    if source_revision is None:
        raise RuntimeError("The install-flow scenario requires a source revision to build.")

    source_checkout_dir = context.temp_root / "source-checkout"
    run(
        ["git", "clone", "--no-checkout", str(REPO_ROOT), str(source_checkout_dir)],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    run(
        ["git", "-C", str(source_checkout_dir), "checkout", "--detach", source_revision],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    context.source_checkout_dir = source_checkout_dir
    context.resolved_commit = (
        run(
            ["git", "-C", str(source_checkout_dir), "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
            env=context.host_tool_env(),
            capture_output=True,
        )
        .stdout.strip()
    )


def build_backend_image(context: SmokeContext) -> None:
    prepare_source_checkout(context)
    if context.source_checkout_dir is None:
        raise RuntimeError("The install-flow scenario did not prepare the source checkout.")

    context.short_revision = (
        run(
            ["git", "-C", str(context.source_checkout_dir), "rev-parse", "--short", "HEAD"],
            cwd=REPO_ROOT,
            env=context.host_tool_env(),
            capture_output=True,
        )
        .stdout.strip()
    )
    context.image_tag = f"track-install-smoke:{context.short_revision}"
    context.fixture_container_name = f"track-install-smoke-{int(time.time())}"

    run(
        [
            "docker",
            "build",
            "-t",
            context.image_tag,
            "--build-arg",
            f"TRACK_GIT_COMMIT={context.short_revision}",
            str(context.source_checkout_dir),
        ],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )
    run(
        ["docker", "image", "inspect", context.image_tag],
        cwd=REPO_ROOT,
        env=context.host_tool_env(),
    )


def install_track(context: SmokeContext) -> None:
    if context.revision is None:
        raise RuntimeError("The install-flow scenario requires a git revision.")
    if context.image_tag is None:
        raise RuntimeError("Backend image tag is not prepared.")

    run(
        [str(TRACKUP_PATH), "--default", "--ref", context.revision],
        cwd=REPO_ROOT,
        env=context.smoke_env(
            {
                "TRACKUP_IMAGE_REF": context.image_tag,
                **(
                    {"TRACKUP_EXPECTED_COMMIT": context.expected_commit}
                    if context.expected_commit is not None
                    else {}
                ),
            }
        ),
    )


def start_backend(context: SmokeContext) -> None:
    if context.api_base_url is None:
        raise RuntimeError("The install-flow scenario did not prepare the API base URL.")

    run(
        [str(context.track_backend_path), "up", "-d"],
        cwd=REPO_ROOT,
        env=context.backend_env(),
    )
    context.backend_running = True

    api = TrackApiClient(context.api_base_url)
    wait_until(
        "the packaged backend health endpoint",
        api.health_ok,
        timeout_seconds=30,
    )

    if "<html" not in api.index_html().lower():
        raise RuntimeError("The packaged backend did not serve the bundled frontend HTML.")


def configure_cli(context: SmokeContext) -> None:
    if context.install_flow_options is None:
        raise RuntimeError("The install-flow scenario did not initialize its options.")

    command = [str(context.track_cli_path), "configure"]
    if context.install_flow_options.configure_cli_with_backend_url:
        if context.api_base_url is None:
            raise RuntimeError("The install-flow scenario did not prepare the API base URL.")
        command.extend(["--backend-url", context.api_base_url])

    run(command, cwd=REPO_ROOT, env=context.smoke_env())


def register_project_checkout(context: SmokeContext) -> None:
    build_project_checkout(context.project_checkout)
    run(
        [
            str(context.track_cli_path),
            "project",
            "register",
            str(context.project_checkout),
        ],
        cwd=REPO_ROOT,
        env=context.smoke_env(),
    )


def align_project_metadata_with_fixture(context: SmokeContext) -> None:
    if context.fixture_repository is None:
        raise RuntimeError("The install-flow scenario did not seed fixture repository metadata.")
    if context.api_base_url is None:
        raise RuntimeError("The install-flow scenario did not prepare the API base URL.")

    # The smoke still uses the real `track project register` path so repo
    # discovery exercises the installed CLI, but the remote fixture cannot
    # reach a real GitHub SSH URL during CI. We therefore patch only the Git
    # transport URL through the public API so the registered project keeps its
    # GitHub-facing repo URL while remote `git fetch upstream` stays inside the
    # seeded local fixture repository.
    api = TrackApiClient(context.api_base_url)
    existing_project = api.project(canonical_name=PROJECT_NAME)
    existing_metadata = existing_project["metadata"]
    api.update_project_metadata(
        canonical_name=PROJECT_NAME,
        repo_url=str(existing_metadata["repoUrl"]),
        git_url=context.fixture_upstream_git_url(),
        base_branch=str(existing_metadata["baseBranch"]),
        description=existing_metadata.get("description"),
    )


def configure_remote_agent(context: SmokeContext) -> None:
    if context.fixture_port is None:
        raise RuntimeError("The install-flow scenario did not reserve a fixture port.")
    if context.install_flow_options is None:
        raise RuntimeError("The install-flow scenario did not initialize its options.")

    command = [
        str(context.track_cli_path),
        "remote-agent",
        "configure",
        "--host",
        context.remote_agent_host(),
        "--user",
        context.remote_agent_user(),
        "--port",
        str(context.remote_agent_port()),
    ]
    if context.install_flow_options.remote_agent_workspace_root is not None:
        command.extend(
            ["--workspace-root", context.install_flow_options.remote_agent_workspace_root]
        )
    if context.install_flow_options.remote_agent_projects_registry_path is not None:
        command.extend(
            [
                "--projects-registry-path",
                context.install_flow_options.remote_agent_projects_registry_path,
            ]
        )
    command.extend(
        [
            "--identity-file",
            str(context.fixture_key_prefix),
            "--known-hosts-file",
            str(context.fixture_runtime_dir / "known_hosts"),
            "--shell-prelude",
            context.fixture_shell_prelude(),
            "--enable-review-follow-up",
            "--main-user",
            REVIEW_MAIN_USER,
            "--default-review-prompt",
            "Focus on regressions, broken flows, and missing tests.",
        ]
    )

    run(command, cwd=REPO_ROOT, env=context.smoke_env())


def build_project_checkout(checkout_path: Path) -> None:
    checkout_path.mkdir(parents=True, exist_ok=True)
    run(["git", "init", "-b", "main", str(checkout_path)], cwd=REPO_ROOT)
    run(["git", "-C", str(checkout_path), "config", "user.name", "Track Smoke"], cwd=REPO_ROOT)
    run(
        [
            "git",
            "-C",
            str(checkout_path),
            "config",
            "user.email",
            "track-smoke@example.com",
        ],
        cwd=REPO_ROOT,
    )
    write_text(
        checkout_path / "README.md",
        f"# {PROJECT_NAME}\n\nScratch checkout for the install smoke test.\n",
    )
    run(["git", "-C", str(checkout_path), "add", "README.md"], cwd=REPO_ROOT)
    run(
        ["git", "-C", str(checkout_path), "commit", "-m", "chore: seed smoke repo"],
        cwd=REPO_ROOT,
    )
    run(["git", "-C", str(checkout_path), "remote", "add", "origin", PROJECT_GIT_URL], cwd=REPO_ROOT)
