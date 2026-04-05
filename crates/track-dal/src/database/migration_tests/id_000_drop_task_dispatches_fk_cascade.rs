use sqlx::{sqlite::SqliteRow, Row};

use crate::database::DatabaseContext;

use super::support::{execute_statements, temporary_database};

#[derive(Debug, PartialEq, Eq)]
struct TaskDispatchSnapshot {
    dispatch_id: String,
    task_id: String,
    project: String,
    status: String,
    created_at: String,
    updated_at: String,
    finished_at: Option<String>,
    remote_host: String,
    branch_name: Option<String>,
    worktree_path: Option<String>,
    pull_request_url: Option<String>,
    preferred_tool: String,
    follow_up_request: Option<String>,
    summary: Option<String>,
    notes: Option<String>,
    error_message: Option<String>,
    review_request_head_oid: Option<String>,
    review_request_user: Option<String>,
}

impl TaskDispatchSnapshot {
    fn from_row(row: SqliteRow) -> Self {
        Self {
            dispatch_id: row.get("dispatch_id"),
            task_id: row.get("task_id"),
            project: row.get("project"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            finished_at: row.get("finished_at"),
            remote_host: row.get("remote_host"),
            branch_name: row.get("branch_name"),
            worktree_path: row.get("worktree_path"),
            pull_request_url: row.get("pull_request_url"),
            preferred_tool: row.get("preferred_tool"),
            follow_up_request: row.get("follow_up_request"),
            summary: row.get("summary"),
            notes: row.get("notes"),
            error_message: row.get("error_message"),
            review_request_head_oid: row.get("review_request_head_oid"),
            review_request_user: row.get("review_request_user"),
        }
    }
}

#[tokio::test]
async fn preserves_rows_and_values_when_task_dispatches_is_rebuilt_without_fk_cascade() {
    let (_directory, database) = temporary_database();

    // First, recreate the exact table shape that production databases had
    // immediately before this migration: the cascade FK is still present, and
    // `preferred_tool` has not been added yet.
    database
        .run(|connection| {
            Box::pin(async move {
                execute_statements(
                    connection,
                    &[
                        r#"
                        CREATE TABLE projects (
                            canonical_name TEXT PRIMARY KEY,
                            repo_url TEXT NOT NULL DEFAULT '',
                            git_url TEXT NOT NULL DEFAULT '',
                            base_branch TEXT NOT NULL DEFAULT 'main',
                            description TEXT
                        )
                        "#,
                        r#"
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
                        )
                        "#,
                        r#"
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
                            follow_up_request TEXT,
                            summary TEXT,
                            notes TEXT,
                            error_message TEXT,
                            review_request_head_oid TEXT,
                            review_request_user TEXT,
                            FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
                        )
                        "#,
                    ],
                    "legacy schema should be created",
                )
                .await;

                // Next, seed representative rows that cover the bug-prone
                // cases: one dispatch with a NULL follow-up request and one
                // follow-up row whose tail columns are all populated.
                sqlx::query(
                    r#"
                    INSERT INTO projects (canonical_name, repo_url, git_url, base_branch, description)
                    VALUES ('project-a', 'https://github.com/acme/project-a', 'git@github.com:acme/project-a.git', 'main', NULL)
                    "#,
                )
                .execute(&mut *connection)
                .await
                .expect("project should be inserted");

                sqlx::query(
                    r#"
                    INSERT INTO tasks (id, project, priority, status, description, created_at, updated_at, source)
                    VALUES ('task-1', 'project-a', 'high', 'open', 'Legacy task', '2026-04-01T10:00:00.000Z', '2026-04-01T10:00:00.000Z', 'web')
                    "#,
                )
                .execute(&mut *connection)
                .await
                .expect("task should be inserted");

                sqlx::query(
                    r#"
                    INSERT INTO task_dispatches (
                        dispatch_id, task_id, project, status, created_at, updated_at,
                        finished_at, remote_host, branch_name, worktree_path,
                        pull_request_url, follow_up_request, summary, notes,
                        error_message, review_request_head_oid, review_request_user
                    )
                    VALUES (
                        'dispatch-1', 'task-1', 'project-a', 'succeeded',
                        '2026-04-01T10:00:00.000Z', '2026-04-01T10:05:00.000Z',
                        '2026-04-01T10:05:00.000Z', 'builder.example',
                        'track/dispatch-1', '~/workspace/project-a/worktrees/dispatch-1',
                        'https://github.com/acme/project-a/pull/1', NULL,
                        'Initial remote run', NULL, NULL, NULL, NULL
                    )
                    "#,
                )
                .execute(&mut *connection)
                .await
                .expect("initial dispatch should be inserted");

                sqlx::query(
                    r#"
                    INSERT INTO task_dispatches (
                        dispatch_id, task_id, project, status, created_at, updated_at,
                        finished_at, remote_host, branch_name, worktree_path,
                        pull_request_url, follow_up_request, summary, notes,
                        error_message, review_request_head_oid, review_request_user
                    )
                    VALUES (
                        'dispatch-2', 'task-1', 'project-a', 'failed',
                        '2026-04-01T11:00:00.000Z', '2026-04-01T11:07:00.000Z',
                        '2026-04-01T11:07:00.000Z', 'builder.example',
                        'track/dispatch-1', '~/workspace/project-a/worktrees/dispatch-1',
                        'https://github.com/acme/project-a/pull/1',
                        'Respond to new review feedback from @popzxc on the existing PR.',
                        'Follow-up request: Respond to new review feedback from @popzxc on the existing PR.',
                        'Saved note', 'Saved error', 'head-1', 'popzxc'
                    )
                    "#,
                )
                .execute(&mut *connection)
                .await
                .expect("follow-up dispatch should be inserted");

                Ok(())
            })
        }).await
        .expect("legacy state should be prepared");

    database
        .initialize()
        .await
        .expect("database should initialize");

    // Finally, assert both the schema change and the observable row contents.
    // This keeps the test focused on the migration contract: all rows survive,
    // and no column values slide into a neighbor during the table rebuild.
    assert_eq!(
        load_task_dispatch_snapshots(&database).await,
        vec![
            TaskDispatchSnapshot {
                dispatch_id: "dispatch-1".to_owned(),
                task_id: "task-1".to_owned(),
                project: "project-a".to_owned(),
                status: "succeeded".to_owned(),
                created_at: "2026-04-01T10:00:00.000Z".to_owned(),
                updated_at: "2026-04-01T10:05:00.000Z".to_owned(),
                finished_at: Some("2026-04-01T10:05:00.000Z".to_owned()),
                remote_host: "builder.example".to_owned(),
                branch_name: Some("track/dispatch-1".to_owned()),
                worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
                pull_request_url: Some("https://github.com/acme/project-a/pull/1".to_owned()),
                preferred_tool: "codex".to_owned(),
                follow_up_request: None,
                summary: Some("Initial remote run".to_owned()),
                notes: None,
                error_message: None,
                review_request_head_oid: None,
                review_request_user: None,
            },
            TaskDispatchSnapshot {
                dispatch_id: "dispatch-2".to_owned(),
                task_id: "task-1".to_owned(),
                project: "project-a".to_owned(),
                status: "failed".to_owned(),
                created_at: "2026-04-01T11:00:00.000Z".to_owned(),
                updated_at: "2026-04-01T11:07:00.000Z".to_owned(),
                finished_at: Some("2026-04-01T11:07:00.000Z".to_owned()),
                remote_host: "builder.example".to_owned(),
                branch_name: Some("track/dispatch-1".to_owned()),
                worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
                pull_request_url: Some("https://github.com/acme/project-a/pull/1".to_owned()),
                preferred_tool: "codex".to_owned(),
                follow_up_request: Some(
                    "Respond to new review feedback from @popzxc on the existing PR.".to_owned(),
                ),
                summary: Some(
                    "Follow-up request: Respond to new review feedback from @popzxc on the existing PR."
                        .to_owned(),
                ),
                notes: Some("Saved note".to_owned()),
                error_message: Some("Saved error".to_owned()),
                review_request_head_oid: Some("head-1".to_owned()),
                review_request_user: Some("popzxc".to_owned()),
            },
        ]
    );
    assert!(
        !load_task_dispatch_schema_sql(&database)
            .await
            .to_uppercase()
            .contains("ON DELETE CASCADE"),
        "task_dispatches should no longer use ON DELETE CASCADE",
    );
}

async fn load_task_dispatch_snapshots(database: &DatabaseContext) -> Vec<TaskDispatchSnapshot> {
    database
        .run(|connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT
                        dispatch_id,
                        task_id,
                        project,
                        status,
                        created_at,
                        updated_at,
                        finished_at,
                        remote_host,
                        branch_name,
                        worktree_path,
                        pull_request_url,
                        preferred_tool,
                        follow_up_request,
                        summary,
                        notes,
                        error_message,
                        review_request_head_oid,
                        review_request_user
                    FROM task_dispatches
                    ORDER BY dispatch_id
                    "#,
                )
                .fetch_all(&mut *connection)
                .await
                .expect("migrated dispatches should load");

                Ok(rows
                    .into_iter()
                    .map(TaskDispatchSnapshot::from_row)
                    .collect())
            })
        })
        .await
        .expect("migrated rows should be returned")
}

async fn load_task_dispatch_schema_sql(database: &DatabaseContext) -> String {
    database
        .run(|connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'task_dispatches'",
                )
                .fetch_one(&mut *connection)
                .await
                .expect("task_dispatches schema should load");

                Ok(row.get::<String, _>("sql"))
            })
        }).await
        .expect("task_dispatches schema should be returned")
}
