import json

from ..api_client import TrackApiClient
from ..constants import PROJECT_NAME, REPO_ROOT
from ..shell_utils import run, wait_until
from ..smoke_context import SmokeContext


# ==============================================================================
# Shared Task Flow
# ==============================================================================
#
# Once the installed stack is live, both platform variants should exercise the
# same observable user flow. Keeping those actions together makes it obvious
# which parts of the smoke are true product behavior rather than setup glue.


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
