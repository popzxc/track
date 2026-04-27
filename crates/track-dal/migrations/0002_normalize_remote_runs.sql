-- =============================================================================
-- Normalize Remote Run Lifecycle
-- =============================================================================
--
-- Task dispatches and review runs have separate domain payloads, but they
-- share one lifecycle: preferred tool, status, timestamps, remote layout, and
-- terminal outcome fields. Persisting that lifecycle once keeps the database
-- shape aligned with the Rust model.

CREATE TABLE remote_runs (
    dispatch_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('task', 'review')),
    preferred_tool TEXT NOT NULL DEFAULT 'codex',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    remote_host TEXT NOT NULL,
    branch_name TEXT,
    worktree_path TEXT,
    follow_up_request TEXT,
    summary TEXT,
    notes TEXT,
    error_message TEXT
);

CREATE TABLE task_run_details (
    dispatch_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    project TEXT NOT NULL,
    pull_request_url TEXT,
    review_request_head_oid TEXT,
    review_request_user TEXT,
    FOREIGN KEY (dispatch_id) REFERENCES remote_runs(dispatch_id) ON DELETE CASCADE
);

CREATE TABLE review_run_details (
    dispatch_id TEXT PRIMARY KEY,
    review_id TEXT NOT NULL,
    pull_request_url TEXT NOT NULL,
    repository_full_name TEXT NOT NULL,
    workspace_key TEXT NOT NULL,
    target_head_oid TEXT,
    review_submitted INTEGER NOT NULL DEFAULT 0,
    github_review_id TEXT,
    github_review_url TEXT,
    FOREIGN KEY (dispatch_id) REFERENCES remote_runs(dispatch_id) ON DELETE CASCADE,
    FOREIGN KEY (review_id) REFERENCES reviews(id) ON DELETE CASCADE
);

CREATE INDEX idx_remote_runs_kind_created_at
ON remote_runs(kind, created_at DESC);

CREATE INDEX idx_remote_runs_status_created_at
ON remote_runs(status, created_at DESC);

CREATE INDEX idx_task_run_details_task_id
ON task_run_details(task_id);

CREATE INDEX idx_review_run_details_review_id
ON review_run_details(review_id);

INSERT INTO remote_runs (
    dispatch_id,
    kind,
    preferred_tool,
    status,
    created_at,
    updated_at,
    finished_at,
    remote_host,
    branch_name,
    worktree_path,
    follow_up_request,
    summary,
    notes,
    error_message
)
SELECT
    dispatch_id,
    'task',
    preferred_tool,
    status,
    created_at,
    updated_at,
    finished_at,
    remote_host,
    branch_name,
    worktree_path,
    follow_up_request,
    summary,
    notes,
    error_message
FROM task_dispatches;

INSERT INTO task_run_details (
    dispatch_id,
    task_id,
    project,
    pull_request_url,
    review_request_head_oid,
    review_request_user
)
SELECT
    dispatch_id,
    task_id,
    project,
    pull_request_url,
    review_request_head_oid,
    review_request_user
FROM task_dispatches;

INSERT INTO remote_runs (
    dispatch_id,
    kind,
    preferred_tool,
    status,
    created_at,
    updated_at,
    finished_at,
    remote_host,
    branch_name,
    worktree_path,
    follow_up_request,
    summary,
    notes,
    error_message
)
SELECT
    dispatch_id,
    'review',
    preferred_tool,
    status,
    created_at,
    updated_at,
    finished_at,
    remote_host,
    branch_name,
    worktree_path,
    follow_up_request,
    summary,
    notes,
    error_message
FROM review_runs;

INSERT INTO review_run_details (
    dispatch_id,
    review_id,
    pull_request_url,
    repository_full_name,
    workspace_key,
    target_head_oid,
    review_submitted,
    github_review_id,
    github_review_url
)
SELECT
    dispatch_id,
    review_id,
    pull_request_url,
    repository_full_name,
    workspace_key,
    target_head_oid,
    review_submitted,
    github_review_id,
    github_review_url
FROM review_runs;

DROP TABLE task_dispatches;
DROP TABLE review_runs;
