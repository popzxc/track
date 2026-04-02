from urllib.parse import quote, urlencode

import requests


class TrackApiClient:
    """Talk to the installed API exactly as an external client would."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()

    def request(
        self,
        path: str,
        *,
        method: str = "GET",
        payload: dict | None = None,
        expected_status: int = 200,
        return_text: bool = False,
    ):
        response = self.session.request(
            method=method,
            url=f"{self.base_url}{path}",
            json=payload,
            timeout=5,
        )
        if response.status_code != expected_status:
            raise RuntimeError(
                f"Unexpected status from {method} {path}: {response.status_code}\n{response.text}"
            )
        if return_text:
            return response.text
        return response.json()

    def health_ok(self) -> bool:
        return self.request("/health")["ok"] is True

    def index_html(self) -> str:
        return self.request("/", return_text=True)

    def latest_task_for_title(self, *, project: str, title: str) -> dict:
        response = self.request(f"/api/tasks?{urlencode({'project': project})}")
        for task in response["tasks"]:
            if task["description"].splitlines()[0].strip() == title:
                return task
        raise RuntimeError(f"Could not find a task whose title is {title!r}.")

    def latest_dispatch_for_task(self, *, task_id: str) -> dict | None:
        response = self.request(f"/api/dispatches?{urlencode([('taskId', task_id)])}")
        dispatches = response["dispatches"]
        if not dispatches:
            return None
        return dispatches[0]

    def latest_review_run(self, *, review_id: str) -> dict | None:
        response = self.request(f"/api/reviews/{quote(review_id, safe='')}/runs")
        runs = response["runs"]
        if not runs:
            return None
        return runs[0]

    def dispatch_task(self, *, task_id: str) -> None:
        self.request(
            f"/api/tasks/{quote(task_id, safe='')}/dispatch",
            method="POST",
            payload={},
        )

    def create_review(self, *, pull_request_url: str, extra_instructions: str) -> dict:
        return self.request(
            "/api/reviews",
            method="POST",
            payload={
                "pullRequestUrl": pull_request_url,
                "extraInstructions": extra_instructions,
            },
        )

    def close_task(self, *, task_id: str) -> dict:
        return self.request(
            f"/api/tasks/{quote(task_id, safe='')}",
            method="PATCH",
            payload={"status": "closed"},
        )

    def tasks(self, *, project: str, include_closed: bool = False) -> list[dict]:
        params = {"project": project}
        if include_closed:
            params["includeClosed"] = "true"
        response = self.request(f"/api/tasks?{urlencode(params)}")
        return response["tasks"]
