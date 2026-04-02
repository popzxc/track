from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURECTL_PATH = REPO_ROOT / "testing" / "support" / "fixturectl.py"
TRACKUP_PATH = REPO_ROOT / "trackup" / "trackup"

FIXTURE_IMAGE = "track-testing/ssh-fixture:install-smoke"
FIXTURE_HOST = "127.0.0.1"
FIXTURE_USER = "fixture-user"
FIXTURE_WORKSPACE_ROOT = "/home/track/workspace"
FIXTURE_PROJECTS_REGISTRY_PATH = "/srv/track-testing/state/track-projects.json"
FIXTURE_SHELL_PRELUDE = (
    'export PATH="/opt/track-testing/bin:$PATH"\n'
    'export TRACK_TESTING_RUNTIME_DIR="/srv/track-testing"'
)
MACOS_HOST_FIXTURE_WORKSPACE_ROOT = "~/workspace-smoke"
MACOS_HOST_FIXTURE_PROJECTS_REGISTRY_PATH = "~/track-projects-smoke.json"

PROJECT_NAME = "project-a"
PROJECT_REPO_URL = "https://github.com/acme/project-a"
PROJECT_GIT_URL = "git@github.com:acme/project-a.git"
REVIEW_MAIN_USER = "octocat"
TASK_TITLE = "Install smoke task"
