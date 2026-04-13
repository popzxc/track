-- =============================================================================
-- Initial SQLite Schema
-- =============================================================================
--
-- This baseline migration captures the current track schema for new databases.
-- Older handwritten upgrade steps are intentionally not reproduced here because
-- supported user databases are already expected to be on this shape.

CREATE TABLE projects (
    canonical_name TEXT PRIMARY KEY,
    repo_url TEXT NOT NULL DEFAULT '',
    git_url TEXT NOT NULL DEFAULT '',
    base_branch TEXT NOT NULL DEFAULT 'main',
    description TEXT
);

CREATE TABLE project_aliases (
    canonical_name TEXT NOT NULL,
    alias TEXT NOT NULL,
    PRIMARY KEY (canonical_name, alias),
    UNIQUE (alias),
    FOREIGN KEY (canonical_name) REFERENCES projects(canonical_name) ON DELETE CASCADE
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    project TEXT NOT NULL,
    priority TEXT NOT NULL,
    status TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    source TEXT,
    FOREIGN KEY (project) REFERENCES projects(canonical_name) ON DELETE RESTRICT
);

CREATE TABLE task_dispatches (
    dispatch_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    project TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    remote_host TEXT NOT NULL,
    branch_name TEXT,
    worktree_path TEXT,
    pull_request_url TEXT,
    preferred_tool TEXT NOT NULL DEFAULT 'codex',
    follow_up_request TEXT,
    summary TEXT,
    notes TEXT,
    error_message TEXT,
    review_request_head_oid TEXT,
    review_request_user TEXT
);

CREATE INDEX idx_task_dispatches_task_id_created_at
ON task_dispatches(task_id, created_at DESC);

CREATE TABLE reviews (
    id TEXT PRIMARY KEY,
    pull_request_url TEXT NOT NULL,
    pull_request_number INTEGER NOT NULL,
    pull_request_title TEXT NOT NULL,
    repository_full_name TEXT NOT NULL,
    repo_url TEXT NOT NULL,
    git_url TEXT NOT NULL,
    base_branch TEXT NOT NULL,
    workspace_key TEXT NOT NULL,
    preferred_tool TEXT NOT NULL DEFAULT 'codex',
    project TEXT,
    main_user TEXT NOT NULL,
    default_review_prompt TEXT,
    extra_instructions TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE review_runs (
    dispatch_id TEXT PRIMARY KEY,
    review_id TEXT NOT NULL,
    pull_request_url TEXT NOT NULL,
    repository_full_name TEXT NOT NULL,
    workspace_key TEXT NOT NULL,
    preferred_tool TEXT NOT NULL DEFAULT 'codex',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    remote_host TEXT NOT NULL,
    branch_name TEXT,
    worktree_path TEXT,
    follow_up_request TEXT,
    target_head_oid TEXT,
    summary TEXT,
    review_submitted INTEGER NOT NULL DEFAULT 0,
    github_review_id TEXT,
    github_review_url TEXT,
    notes TEXT,
    error_message TEXT,
    FOREIGN KEY (review_id) REFERENCES reviews(id) ON DELETE CASCADE
);

CREATE INDEX idx_review_runs_review_id_created_at
ON review_runs(review_id, created_at DESC);

CREATE TABLE backend_settings (
    setting_key TEXT PRIMARY KEY,
    setting_json TEXT NOT NULL
);
