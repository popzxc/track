import json
import os
import shutil
import sys
import time
from pathlib import Path

import requests

from .api_client import TrackApiClient
from .constants import (
    FIXTURECTL_PATH,
    FIXTURE_HOST,
    FIXTURE_IMAGE,
    FIXTURE_PROJECTS_REGISTRY_PATH,
    FIXTURE_SHELL_PRELUDE,
    FIXTURE_USER,
    FIXTURE_WORKSPACE_ROOT,
    PROJECT_GIT_URL,
    PROJECT_NAME,
    PROJECT_REPO_URL,
    REVIEW_MAIN_USER,
    REPO_ROOT,
    TRACKUP_PATH,
)
from .shell_utils import reserve_local_port, run, run_json, wait_until, write_text
from .smoke_context import InstallFlowOptions, SmokeContext


# ==============================================================================
# Safety And Fixture Preparation
# ==============================================================================
#
# The smoke is intentionally close to a real installed flow, which means it can
# touch Docker, SSH fixture state, and installer-managed files. We therefore
# reject local runs up front and keep every other mutable artifact inside a
# throwaway temp root.


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


def apply_install_flow_defaults(context: SmokeContext) -> None:
    # The default-path scenario intentionally omits the backend URL, workspace
    # root, and remote registry overrides so we exercise the same values a user
    # would get from a fresh install without extra flags.
    context.install_flow_options = InstallFlowOptions(
        use_default_backend_port=True,
        configure_cli_with_backend_url=False,
        remote_agent_workspace_root=None,
        remote_agent_projects_registry_path=None,
    )


def apply_install_flow_overrides(context: SmokeContext) -> None:
    # The override-path scenario keeps the explicit values so we continue to
    # verify that non-default configuration is respected end to end.
    context.install_flow_options = InstallFlowOptions(
        use_default_backend_port=False,
        configure_cli_with_backend_url=True,
        remote_agent_workspace_root=FIXTURE_WORKSPACE_ROOT,
        remote_agent_projects_registry_path=FIXTURE_PROJECTS_REGISTRY_PATH,
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
    )
    run(
        ["git", "-C", str(source_checkout_dir), "checkout", "--detach", source_revision],
        cwd=REPO_ROOT,
    )
    context.source_checkout_dir = source_checkout_dir
    context.resolved_commit = (
        run(
            ["git", "-C", str(source_checkout_dir), "rev-parse", "HEAD"],
            cwd=REPO_ROOT,
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
    )
    run(["docker", "image", "inspect", context.image_tag], cwd=REPO_ROOT)


def start_fixture(context: SmokeContext) -> None:
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
        ],
        cwd=REPO_ROOT,
    )
    context.fixture_running = True

    run_json(
        [
            "python3",
            str(FIXTURECTL_PATH),
            "wait-for-ssh",
            "--host",
            FIXTURE_HOST,
            "--user",
            FIXTURE_USER,
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
    )
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
            FIXTURE_USER,
            "--login",
            FIXTURE_USER,
        ],
        cwd=REPO_ROOT,
    )

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


# ==============================================================================
# Installed Stack Setup
# ==============================================================================
#
# Once the fixture is ready, we switch to the exact path a user would take:
# install through trackup, start the packaged backend, and configure the
# installed CLI against that live backend.


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
        FIXTURE_HOST,
        "--user",
        FIXTURE_USER,
        "--port",
        str(context.fixture_port),
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
            FIXTURE_SHELL_PRELUDE,
            "--enable-review-follow-up",
            "--main-user",
            REVIEW_MAIN_USER,
            "--default-review-prompt",
            "Focus on regressions, broken flows, and missing tests.",
        ]
    )

    run(command, cwd=REPO_ROOT, env=context.smoke_env())


# ==============================================================================
# Task Flow
# ==============================================================================
#
# The smoke stays deterministic by feeding a pre-baked inference result into the
# hidden capture seam. From there onward we use the live API and remote fixture
# exactly as the installed stack would in production.


def capture_task(context: SmokeContext) -> None:
    candidate = {
        "project": PROJECT_NAME,
        "priority": "high",
        "title": context.task_title,
        "bodyMarkdown": (
            "- Verify the installed stack can capture, dispatch, review, and close a task."
        ),
        "confidence": "high",
    }
    capture_note = (
        f"{PROJECT_NAME} prio high verify the installed stack can capture, dispatch, "
        "review, and close a task"
    )
    run(
        [
            str(context.track_cli_path),
            capture_note,
        ],
        cwd=REPO_ROOT,
        env=context.smoke_env(
            {
                "TRACK_TEST_INFERENCE": "1",
                "TRACK_TEST_INFERENCE_RESULT": json.dumps(candidate),
            }
        ),
    )

    api = TrackApiClient(context.api_base_url)
    task = wait_until(
        "the captured task to appear in the API",
        lambda: api.latest_task_for_title(project=PROJECT_NAME, title=context.task_title),
        timeout_seconds=15,
    )
    context.task_id = str(task["id"])


def dispatch_task(context: SmokeContext) -> None:
    if context.task_id is None:
        raise RuntimeError("Task id is not available for dispatch.")

    api = TrackApiClient(context.api_base_url)
    api.dispatch_task(task_id=context.task_id)
    dispatch = wait_until(
        "the remote task dispatch to succeed",
        lambda: (
            latest := api.latest_dispatch_for_task(task_id=context.task_id)
        )
        and latest["status"] == "succeeded"
        and latest,
        timeout_seconds=30,
    )
    context.dispatch_id = str(dispatch["dispatchId"])
    context.pull_request_url = str(dispatch["pullRequestUrl"])


def request_review(context: SmokeContext) -> None:
    if context.pull_request_url is None:
        raise RuntimeError("Pull request URL is not available for review.")

    api = TrackApiClient(context.api_base_url)
    review_response = api.create_review(
        pull_request_url=context.pull_request_url,
        extra_instructions=(
            "Double-check the smoke path and confirm nothing is obviously broken."
        ),
    )
    context.review_id = str(review_response["review"]["id"])
    review_run = wait_until(
        "the PR review run to succeed",
        lambda: (
            latest := api.latest_review_run(review_id=context.review_id)
        )
        and latest["status"] == "succeeded"
        and latest.get("reviewSubmitted") is True
        and latest,
        timeout_seconds=30,
    )
    if review_run["githubReviewId"] != "42001":
        raise RuntimeError(
            f"Expected the smoke review to submit review id 42001, got {review_run['githubReviewId']!r}."
        )


def close_task(context: SmokeContext) -> None:
    if context.task_id is None:
        raise RuntimeError("Task id is not available for closing.")

    api = TrackApiClient(context.api_base_url)
    closed_task = api.close_task(task_id=context.task_id)
    if closed_task["status"] != "closed":
        raise RuntimeError(f"Expected the task to close, got {closed_task['status']!r}.")

    closed_tasks = api.tasks(project=PROJECT_NAME, include_closed=True)
    if not any(task["id"] == context.task_id and task["status"] == "closed" for task in closed_tasks):
        raise RuntimeError("The closed task did not appear in the includeClosed task listing.")


def print_failure_diagnostics(context: SmokeContext) -> None:
    print("\nSmoke test failed. Collecting diagnostics...", file=sys.stderr)
    if context.track_backend_path.is_file():
        try:
            run(
                [str(context.track_backend_path), "ps"],
                cwd=REPO_ROOT,
                env=context.smoke_env(),
                check=False,
            )
            run(
                [str(context.track_backend_path), "logs", "--no-color"],
                cwd=REPO_ROOT,
                env=context.smoke_env(),
                check=False,
            )
        except Exception as error:  # noqa: BLE001
            print(f"Could not collect backend logs: {error}", file=sys.stderr)

    runtime_logs = context.temp_root / "fixture-runtime" / "logs"
    if runtime_logs.exists():
        for log_path in sorted(runtime_logs.rglob("*")):
            if not log_path.is_file():
                continue
            print(f"\n--- {log_path} ---", file=sys.stderr)
            try:
                print(log_path.read_text(encoding="utf-8"), file=sys.stderr)
            except Exception as error:  # noqa: BLE001
                print(f"Could not read {log_path}: {error}", file=sys.stderr)


def cleanup_environment(context: SmokeContext) -> None:
    if context.backend_running and context.track_backend_path.is_file():
        run(
            [str(context.track_backend_path), "down", "-v", "--remove-orphans"],
            cwd=REPO_ROOT,
            env=context.backend_env(),
            check=False,
        )

    if context.fixture_running and context.fixture_container_name is not None:
        run(
            [
                "python3",
                str(FIXTURECTL_PATH),
                "stop",
                "--name",
                context.fixture_container_name,
            ],
            cwd=REPO_ROOT,
            check=False,
        )

    if context.temp_root.exists():
        shutil.rmtree(context.temp_root, ignore_errors=True)


def print_install_flow_summary(context: SmokeContext) -> None:
    print("\nInstall smoke flow completed successfully.")
    print(f"Task: {context.task_id}")
    print(f"Dispatch: {context.dispatch_id}")
    print(f"Review: {context.review_id}")
