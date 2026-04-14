import json
import os
import subprocess
import sys
from pathlib import Path

from track_remote_helper.common import (
    CODEX_EVENTS_FILE_NAME,
    FINISHED_AT_FILE_NAME,
    LAUNCHER_PID_FILE_NAME,
    PROMPT_FILE_NAME,
    RESULT_FILE_NAME,
    REVIEW_RUN_DIRECTORY_NAME,
    REVIEW_WORKTREE_DIRECTORY_NAME,
    SCHEMA_FILE_NAME,
    STATUS_FILE_NAME,
    STDERR_FILE_NAME,
    CommandError,
    advisory_lock,
    check_command,
    command_succeeds,
    ensure_parent,
    expand_remote_path,
    kill_if_running,
    read_optional_text,
    read_pid,
    remove_path,
    resolve_binary,
    utc_timestamp,
    write_text,
    write_text_with_trailing_newline,
)


def agent_pid_file_name(preferred_tool: str) -> str:
    """Return the PID file name for the given agent tool."""
    return f"{preferred_tool}.pid"

TASK_WORKTREE_DIRECTORY_NAME = "worktrees"


def checkout_lock_name(checkout_path: Path) -> str:
    return f"checkout:{checkout_path}"


def handle_command(command_name: str, request: dict[str, object], helper_path: Path) -> dict[str, object]:
    handlers = {
        "cancel-run": cancel_run,
        "cleanup-orphaned-artifacts": cleanup_orphaned_artifacts,
        "cleanup-review-artifacts": cleanup_review_artifacts,
        "cleanup-review-workspace-caches": cleanup_review_workspace_caches,
        "cleanup-task-artifacts": cleanup_task_artifacts,
        "create-review-worktree": create_review_worktree,
        "create-worktree": create_worktree,
        "ensure-checkout": ensure_checkout,
        "ensure-follow-up-worktree": ensure_follow_up_worktree,
        "fetch-gh-api": fetch_gh_api,
        "github-login": github_login,
        "launch-run": lambda raw: launch_run(raw, helper_path),
        "list-directories": list_directories,
        "post-pr-comment": post_pr_comment,
        "read-run-snapshots": read_run_snapshots,
        "reset-workspace": reset_workspace,
        "write-file": write_file,
    }
    try:
        handler = handlers[command_name]
    except KeyError as error:
        raise CommandError(f"unsupported remote helper command {command_name!r}") from error

    return handler(request)


def github_login(_request: dict[str, object]) -> dict[str, object]:
    completed = check_command(
        [resolve_binary("gh"), "api", "user", "--jq", ".login"],
        capture_output=True,
    )
    login = completed.stdout.strip()
    if not login:
        raise CommandError("remote `gh` authentication did not return a GitHub login")

    return {"login": login}


def fetch_gh_api(request: dict[str, object]) -> dict[str, object]:
    endpoint = str(request["endpoint"])
    completed = check_command([resolve_binary("gh"), "api", endpoint], capture_output=True)
    return {"output": completed.stdout}


def post_pr_comment(request: dict[str, object]) -> dict[str, object]:
    endpoint = str(request["endpoint"])
    body = str(request["body"])
    check_command(
        [resolve_binary("gh"), "api", "--method", "POST", endpoint, "-f", f"body={body}"]
    )
    return {}


def write_file(request: dict[str, object]) -> dict[str, object]:
    path = expand_remote_path(str(request["path"]))
    contents = str(request["contents"])
    write_text(path, contents)
    return {}


def list_directories(request: dict[str, object]) -> dict[str, object]:
    directory_path = expand_remote_path(str(request["path"]))
    if not directory_path.is_dir():
        return {"paths": []}

    paths = sorted(str(entry) for entry in directory_path.iterdir() if entry.is_dir())
    return {"paths": paths}


def ensure_checkout(request: dict[str, object]) -> dict[str, object]:
    repo_url = str(request["repoUrl"])
    repository_name = str(request["repositoryName"])
    git_url = str(request["gitUrl"])
    base_branch = str(request["baseBranch"])
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    github_login_name = str(request["githubLogin"])

    # This checkout is shared by every run for the project, so concurrent
    # launches must serialize the refresh and remote/worktree metadata updates.
    with advisory_lock(checkout_lock_name(checkout_path)):
        checkout_path.parent.mkdir(parents=True, exist_ok=True)

        git_env = os.environ.copy()
        remote_ssh_dir = expand_remote_path("~/.ssh")
        remote_known_hosts_path = remote_ssh_dir / "known_hosts"
        remote_ssh_dir.mkdir(parents=True, exist_ok=True)
        os.chmod(remote_ssh_dir, 0o700)
        remote_known_hosts_path.touch(exist_ok=True)
        os.chmod(remote_known_hosts_path, 0o600)
        git_env["GIT_SSH_COMMAND"] = (
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new "
            f"-o UserKnownHostsFile={remote_known_hosts_path}"
        )

        fork_git_url = resolve_fork_git_url(github_login_name, repository_name)
        if not fork_git_url:
            check_command([resolve_binary("gh"), "repo", "fork", repo_url])
            fork_git_url = resolve_fork_git_url(github_login_name, repository_name)

        if not fork_git_url:
            raise CommandError(
                f"could not determine the fork SSH URL for {github_login_name}/{repository_name}"
            )

        if not (checkout_path / ".git").is_dir():
            check_command(["git", "clone", fork_git_url, str(checkout_path)], env=git_env)

        configure_remote(checkout_path, "origin", fork_git_url)
        configure_remote(checkout_path, "upstream", git_url)

        check_command(["git", "fetch", "origin", "--prune"], cwd=checkout_path, env=git_env)
        check_command(["git", "fetch", "upstream", "--prune"], cwd=checkout_path, env=git_env)

        if git_ref_exists(checkout_path, f"refs/heads/{base_branch}"):
            check_command(["git", "checkout", base_branch], cwd=checkout_path, env=git_env)
        else:
            check_command(
                ["git", "checkout", "-B", base_branch, f"upstream/{base_branch}"],
                cwd=checkout_path,
                env=git_env,
            )

        check_command(
            ["git", "reset", "--hard", f"upstream/{base_branch}"],
            cwd=checkout_path,
            env=git_env,
        )
        check_command(["git", "clean", "-fd"], cwd=checkout_path, env=git_env)

    return {"forkGitUrl": fork_git_url}


def create_worktree(request: dict[str, object]) -> dict[str, object]:
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    base_branch = str(request["baseBranch"])
    branch_name = str(request["branchName"])
    worktree_path = expand_remote_path(str(request["worktreePath"]))

    with advisory_lock(checkout_lock_name(checkout_path)):
        prepare_fresh_worktree_path(
            checkout_path,
            worktree_path,
            "Refusing to overwrite unexpected existing path while preparing a fresh dispatch worktree.",
        )
        check_command(
            [
                "git",
                "-C",
                str(checkout_path),
                "worktree",
                "add",
                "-B",
                branch_name,
                str(worktree_path),
                f"upstream/{base_branch}",
            ]
        )
    return {}


def create_review_worktree(request: dict[str, object]) -> dict[str, object]:
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    pull_request_number = int(request["pullRequestNumber"])
    branch_name = str(request["branchName"])
    worktree_path = expand_remote_path(str(request["worktreePath"]))
    target_head_oid = str(request.get("targetHeadOid") or "").strip()

    with advisory_lock(checkout_lock_name(checkout_path)):
        prepare_fresh_worktree_path(
            checkout_path,
            worktree_path,
            "Refusing to overwrite unexpected existing path while preparing a review worktree.",
        )
        check_command(
            [
                "git",
                "-C",
                str(checkout_path),
                "fetch",
                "upstream",
                f"pull/{pull_request_number}/head:{branch_name}",
            ]
        )

        target_ref = branch_name
        if target_head_oid:
            if not git_object_exists(checkout_path, target_head_oid):
                subprocess.run(
                    ["git", "-C", str(checkout_path), "fetch", "upstream", target_head_oid],
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    text=True,
                )

            if git_object_exists(checkout_path, target_head_oid):
                target_ref = target_head_oid
            else:
                completed = check_command(
                    ["git", "-C", str(checkout_path), "rev-parse", f"{branch_name}^{{commit}}"],
                    capture_output=True,
                )
                fetched_head_oid = completed.stdout.strip()
                raise CommandError(
                    f"requested review commit {target_head_oid} is not available locally; "
                    f"the fetched PR head is {fetched_head_oid}"
                )

        check_command(["git", "-C", str(checkout_path), "branch", "-f", branch_name, target_ref])
        check_command(
            [
                "git",
                "-C",
                str(checkout_path),
                "worktree",
                "add",
                "-B",
                branch_name,
                str(worktree_path),
                target_ref,
            ]
        )
    return {}


def ensure_follow_up_worktree(request: dict[str, object]) -> dict[str, object]:
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    branch_name = str(request["branchName"])
    worktree_path = expand_remote_path(str(request["worktreePath"]))

    with advisory_lock(checkout_lock_name(checkout_path)):
        worktree_path.parent.mkdir(parents=True, exist_ok=True)
        fetch_ignore_failure(["git", "-C", str(checkout_path), "fetch", "origin", "--prune"])
        fetch_ignore_failure(["git", "-C", str(checkout_path), "fetch", "upstream", "--prune"])

        if (worktree_path / ".git").exists():
            if not command_succeeds(
                ["git", "-C", str(worktree_path), "rev-parse", "--show-toplevel"]
            ):
                raise CommandError(
                    f"existing follow-up worktree path {worktree_path} is not a valid Git worktree"
                )
            check_command(["git", "-C", str(worktree_path), "checkout", branch_name])
            return {}

        if worktree_path.exists():
            raise CommandError(
                f"follow-up worktree path {worktree_path} already exists but is not a Git worktree"
            )

        check_command(["git", "-C", str(checkout_path), "worktree", "prune"])

        if git_ref_exists(checkout_path, f"refs/heads/{branch_name}"):
            check_command(
                ["git", "-C", str(checkout_path), "worktree", "add", str(worktree_path), branch_name]
            )
            return {}

        if git_ref_exists(checkout_path, f"refs/remotes/origin/{branch_name}"):
            check_command(
                [
                    "git",
                    "-C",
                    str(checkout_path),
                    "worktree",
                    "add",
                    "-B",
                    branch_name,
                    str(worktree_path),
                    f"origin/{branch_name}",
                ]
            )
            return {}

        raise CommandError(f"could not restore the follow-up worktree for branch {branch_name}")


def launch_run(request: dict[str, object], helper_path: Path) -> dict[str, object]:
    run_directory = expand_remote_path(str(request["runDirectory"]))
    worktree_path = expand_remote_path(str(request["worktreePath"]))
    preferred_tool = str(request["preferredTool"])
    shell_prelude = str(request.get("shellPrelude") or "")

    run_directory.mkdir(parents=True, exist_ok=True)
    for filename in [
        STATUS_FILE_NAME,
        RESULT_FILE_NAME,
        STDERR_FILE_NAME,
        FINISHED_AT_FILE_NAME,
        LAUNCHER_PID_FILE_NAME,
        agent_pid_file_name(preferred_tool),
        CODEX_EVENTS_FILE_NAME,
    ]:
        remove_path(run_directory / filename)

    worker_config_path = run_directory / "worker-config.json"
    worker_config_path.write_text(
        json.dumps(
            {
                "preferredTool": preferred_tool,
                "runDirectory": str(run_directory),
                "shellPrelude": shell_prelude,
                "worktreePath": str(worktree_path),
            }
        ),
        encoding="utf-8",
    )

    subprocess.Popen(
        [sys.executable, "-B", str(helper_path), "_run-worker", str(worker_config_path)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        cwd=str(run_directory),
    )
    return {}


def cancel_run(request: dict[str, object]) -> dict[str, object]:
    run_directory = expand_remote_path(str(request["runDirectory"]))
    launcher_pid_file = run_directory / LAUNCHER_PID_FILE_NAME
    # For cancel, we need to check all possible agent PID files since we don't know which tool was used
    agent_pid_files = [
        run_directory / "codex.pid",
        run_directory / "claude.pid",
        run_directory / "opencode.pid",
    ]

    kill_if_running(read_pid(launcher_pid_file))
    for agent_pid_file in agent_pid_files:
        kill_if_running(read_pid(agent_pid_file))

    run_directory.mkdir(parents=True, exist_ok=True)
    write_text_with_trailing_newline(run_directory / STATUS_FILE_NAME, "canceled")
    write_text_with_trailing_newline(run_directory / FINISHED_AT_FILE_NAME, utc_timestamp())
    return {}


def read_run_snapshots(request: dict[str, object]) -> dict[str, object]:
    snapshots = []
    for raw_run_directory in request["runDirectories"]:
        run_directory = expand_remote_path(str(raw_run_directory))
        snapshots.append(
            {
                "runDirectory": str(raw_run_directory),
                "status": read_optional_text(run_directory / STATUS_FILE_NAME),
                "result": read_optional_text(run_directory / RESULT_FILE_NAME),
                "stderr": read_optional_text(run_directory / STDERR_FILE_NAME),
                "finishedAt": read_optional_text(run_directory / FINISHED_AT_FILE_NAME),
            }
        )
    return {"snapshots": snapshots}


def cleanup_task_artifacts(request: dict[str, object]) -> dict[str, object]:
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    worktree_paths = [expand_remote_path(str(path)) for path in request["worktreePaths"]]
    run_directories = [expand_remote_path(str(path)) for path in request["runDirectories"]]
    cleanup_remote_dispatch_directories = str(request["cleanupMode"]) == "deleteTask"

    worktrees_removed = 0
    run_directories_removed = 0

    for run_directory in run_directories:
        kill_run_directory_processes(run_directory)
        status_file = run_directory / STATUS_FILE_NAME
        current_status = (read_optional_text(status_file) or "").strip()
        if run_directory.is_dir() and current_status in {"preparing", "running"}:
            write_text_with_trailing_newline(status_file, "canceled")
            write_text_with_trailing_newline(run_directory / FINISHED_AT_FILE_NAME, utc_timestamp())

    for worktree_path in worktree_paths:
        had_worktree_path = worktree_path.exists()
        if checkout_has_git(checkout_path) and worktree_is_registered(checkout_path, worktree_path):
            subprocess.run(
                ["git", "-C", str(checkout_path), "worktree", "remove", "--force", str(worktree_path)],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )

        if worktree_path.exists():
            remove_path(worktree_path)

        if had_worktree_path and not worktree_path.exists():
            worktrees_removed += 1

    if checkout_has_git(checkout_path):
        subprocess.run(
            ["git", "-C", str(checkout_path), "worktree", "prune"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )

    if cleanup_remote_dispatch_directories:
        for run_directory in run_directories:
            had_run_directory = run_directory.exists()
            if had_run_directory:
                remove_path(run_directory)
            if had_run_directory and not run_directory.exists():
                run_directories_removed += 1

    return {
        "worktreesRemoved": worktrees_removed,
        "runDirectoriesRemoved": run_directories_removed,
    }


def cleanup_review_artifacts(request: dict[str, object]) -> dict[str, object]:
    checkout_path = expand_remote_path(str(request["checkoutPath"]))
    branch_names = [str(name) for name in request["branchNames"]]
    worktree_paths = [expand_remote_path(str(path)) for path in request["worktreePaths"]]
    run_directories = [expand_remote_path(str(path)) for path in request["runDirectories"]]

    worktrees_removed = 0
    run_directories_removed = 0

    for run_directory in run_directories:
        had_run_directory = run_directory.exists()
        kill_run_directory_processes(run_directory)
        if had_run_directory:
            remove_path(run_directory)
        if had_run_directory and not run_directory.exists():
            run_directories_removed += 1

    for worktree_path in worktree_paths:
        had_worktree_path = worktree_path.exists()
        if checkout_has_git(checkout_path) and worktree_is_registered(checkout_path, worktree_path):
            subprocess.run(
                ["git", "-C", str(checkout_path), "worktree", "remove", "--force", str(worktree_path)],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )
        if worktree_path.exists():
            remove_path(worktree_path)
        if had_worktree_path and not worktree_path.exists():
            worktrees_removed += 1

    if checkout_has_git(checkout_path):
        for branch_name in branch_names:
            subprocess.run(
                ["git", "-C", str(checkout_path), "branch", "-D", branch_name],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )
        subprocess.run(
            ["git", "-C", str(checkout_path), "worktree", "prune"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )

    return {
        "worktreesRemoved": worktrees_removed,
        "runDirectoriesRemoved": run_directories_removed,
    }


def cleanup_orphaned_artifacts(request: dict[str, object]) -> dict[str, object]:
    workspace_root = expand_remote_path(str(request["workspaceRoot"]))
    keep_worktree_paths = {str(expand_remote_path(str(path))) for path in request["keepWorktreePaths"]}
    keep_run_directories = {str(expand_remote_path(str(path))) for path in request["keepRunDirectories"]}

    worktrees_removed = 0
    run_directories_removed = 0

    if not workspace_root.is_dir():
        return {"worktreesRemoved": 0, "runDirectoriesRemoved": 0}

    for project_directory in workspace_root.iterdir():
        if not project_directory.is_dir():
            continue

        for run_directory in iterate_matching_paths(project_directory / "dispatches", "dispatch-*"):
            if str(run_directory) in keep_run_directories:
                continue
            run_directories_removed += remove_run_directory(run_directory)

        for worktree_path in iterate_matching_paths(
            project_directory / TASK_WORKTREE_DIRECTORY_NAME,
            "dispatch-*",
        ):
            if str(worktree_path) in keep_worktree_paths:
                continue
            worktrees_removed += remove_orphan_worktree(worktree_path)

        for run_directory in iterate_matching_paths(
            project_directory / REVIEW_RUN_DIRECTORY_NAME,
            "dispatch-*",
        ):
            if str(run_directory) in keep_run_directories:
                continue
            run_directories_removed += remove_run_directory(run_directory)

        for worktree_path in iterate_matching_paths(
            project_directory / REVIEW_WORKTREE_DIRECTORY_NAME,
            "dispatch-*",
        ):
            if str(worktree_path) in keep_worktree_paths:
                continue
            worktrees_removed += remove_orphan_worktree(worktree_path)

    return {
        "worktreesRemoved": worktrees_removed,
        "runDirectoriesRemoved": run_directories_removed,
    }


def cleanup_review_workspace_caches(request: dict[str, object]) -> dict[str, object]:
    for raw_checkout_path in request["checkoutPaths"]:
        checkout_path = expand_remote_path(str(raw_checkout_path))
        workspace_path = checkout_path.parent

        if checkout_has_git(checkout_path):
            subprocess.run(
                ["git", "-C", str(checkout_path), "worktree", "prune"],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )

        if checkout_path.exists():
            remove_path(checkout_path)

        if workspace_path.is_dir():
            try:
                workspace_path.rmdir()
            except OSError:
                pass

    return {}


def reset_workspace(request: dict[str, object]) -> dict[str, object]:
    workspace_root = expand_remote_path(str(request["workspaceRoot"]))
    registry_path = expand_remote_path(str(request["projectsRegistryPath"]))

    if str(workspace_root) in {"", "/", str(Path.home())}:
        raise CommandError(f"refusing to reset an unsafe remote workspace root at {workspace_root}")

    workspace_root.mkdir(parents=True, exist_ok=True)

    workspace_entries_removed = 0
    for entry in workspace_root.iterdir():
        remove_path(entry)
        if not entry.exists():
            workspace_entries_removed += 1

    registry_removed = False
    if registry_path.exists():
        registry_path.unlink()
        registry_removed = not registry_path.exists()

    return {
        "workspaceEntriesRemoved": workspace_entries_removed,
        "registryRemoved": registry_removed,
    }


def resolve_fork_git_url(github_login_name: str, repository_name: str) -> str:
    completed = subprocess.run(
        [
            resolve_binary("gh"),
            "repo",
            "view",
            f"{github_login_name}/{repository_name}",
            "--json",
            "sshUrl",
            "--jq",
            ".sshUrl",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        return ""

    return completed.stdout.strip()


def configure_remote(checkout_path: Path, remote_name: str, remote_url: str) -> None:
    if command_succeeds(["git", "-C", str(checkout_path), "remote", "get-url", remote_name]):
        check_command(["git", "-C", str(checkout_path), "remote", "set-url", remote_name, remote_url])
    else:
        check_command(["git", "-C", str(checkout_path), "remote", "add", remote_name, remote_url])


def git_ref_exists(checkout_path: Path, git_ref: str) -> bool:
    return command_succeeds(
        ["git", "-C", str(checkout_path), "show-ref", "--verify", "--quiet", git_ref]
    )


def git_object_exists(checkout_path: Path, object_name: str) -> bool:
    return command_succeeds(
        ["git", "-C", str(checkout_path), "cat-file", "-e", f"{object_name}^{{commit}}"]
    )


def checkout_has_git(checkout_path: Path) -> bool:
    return (checkout_path / ".git").exists()


def worktree_is_registered(checkout_path: Path, worktree_path: Path) -> bool:
    completed = check_command(
        ["git", "-C", str(checkout_path), "worktree", "list", "--porcelain"],
        capture_output=True,
    )
    expected_line = f"worktree {worktree_path}"
    return any(line.strip() == expected_line for line in completed.stdout.splitlines())


def prepare_fresh_worktree_path(checkout_path: Path, worktree_path: Path, message: str) -> None:
    worktree_path.parent.mkdir(parents=True, exist_ok=True)
    if worktree_path.exists():
        if worktree_is_registered(checkout_path, worktree_path):
            subprocess.run(
                ["git", "-C", str(checkout_path), "worktree", "remove", "--force", str(worktree_path)],
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                text=True,
            )
        else:
            raise CommandError(f"{message} at {worktree_path}")

    check_command(["git", "-C", str(checkout_path), "worktree", "prune"])


def fetch_ignore_failure(argv: list[str]) -> None:
    subprocess.run(
        argv,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
    )


def kill_run_directory_processes(run_directory: Path) -> None:
    kill_if_running(read_pid(run_directory / LAUNCHER_PID_FILE_NAME))
    # Check all possible agent PID files since we don't know which tool was used
    for agent_pid_name in ["codex.pid", "claude.pid", "opencode.pid"]:
        kill_if_running(read_pid(run_directory / agent_pid_name))


def iterate_matching_paths(root: Path, pattern: str) -> list[Path]:
    if not root.is_dir():
        return []
    return sorted(root.glob(pattern))


def remove_run_directory(run_directory: Path) -> int:
    had_run_directory = run_directory.exists()
    kill_run_directory_processes(run_directory)
    if had_run_directory:
        remove_path(run_directory)
    return 1 if had_run_directory and not run_directory.exists() else 0


def remove_orphan_worktree(worktree_path: Path) -> int:
    project_directory = worktree_path.parent.parent
    checkout_path = project_directory / project_directory.name

    if checkout_has_git(checkout_path):
        subprocess.run(
            ["git", "-C", str(checkout_path), "worktree", "remove", "--force", str(worktree_path)],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        subprocess.run(
            ["git", "-C", str(checkout_path), "worktree", "prune"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            text=True,
        )

    had_worktree_path = worktree_path.exists()
    if had_worktree_path:
        remove_path(worktree_path)
    return 1 if had_worktree_path and not worktree_path.exists() else 0
