mod records;

use std::path::PathBuf;

use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_millis};
use track_types::types::{DispatchStatus, RemoteAgentPreferredTool, Task, TaskDispatchRecord};

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone)]
pub struct DispatchRepository {
    database: DatabaseContext,
}

impl DispatchRepository {
    pub async fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path).await?;
        database.initialize().await?;

        Ok(Self { database })
    }

    // TODO: populate it right away and save
    pub fn create_dispatch(
        &self,
        task: &Task,
        remote_host: &str,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let timestamp = now_utc();
        let record = TaskDispatchRecord {
            dispatch_id: format!("dispatch-{}", timestamp.unix_timestamp_nanos()),
            task_id: task.id.clone(),
            preferred_tool,
            project: task.project.clone(),
            status: DispatchStatus::Preparing,
            created_at: timestamp,
            updated_at: timestamp,
            finished_at: None,
            remote_host: remote_host.to_owned(),
            branch_name: None,
            worktree_path: None,
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };

        // Queue flows fill in branch/worktree context immediately after
        // creating the record. Persisting the half-populated row here makes it
        // visible to concurrent reconciliation before that launch context
        // exists, which can incorrectly release a brand-new dispatch.
        Ok(record)
    }

    pub async fn save_dispatch(&self, record: &TaskDispatchRecord) -> Result<(), TrackError> {
        let record = record.clone();
        validate_single_normal_path_component(
            &record.dispatch_id,
            "Dispatch id",
            ErrorCode::InvalidPathComponent,
        )?;
        validate_single_normal_path_component(
            &record.task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let dispatch_id = record.dispatch_id.as_str();
        let task_id = record.task_id.as_str();
        let preferred_tool = record.preferred_tool.as_str();
        let project = record.project.as_str();
        let status = record.status.as_str();
        let created_at = format_iso_8601_millis(record.created_at);
        let updated_at = format_iso_8601_millis(record.updated_at);
        let finished_at = record.finished_at.map(format_iso_8601_millis);
        let remote_host = record.remote_host.as_str();
        let branch_name = record.branch_name.as_deref();
        let worktree_path = record.worktree_path.as_deref();
        let pull_request_url = record.pull_request_url.as_deref();
        let follow_up_request = record.follow_up_request.as_deref();
        let summary = record.summary.as_deref();
        let notes = record.notes.as_deref();
        let error_message = record.error_message.as_deref();
        let review_request_head_oid = record.review_request_head_oid.as_deref();
        let review_request_user = record.review_request_user.as_deref();
        sqlx::query!(
            r#"
            INSERT INTO task_dispatches (
                dispatch_id, task_id, preferred_tool, project, status, created_at, updated_at,
                finished_at, remote_host, branch_name, worktree_path, pull_request_url,
                follow_up_request, summary, notes, error_message, review_request_head_oid,
                review_request_user
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ON CONFLICT(dispatch_id) DO UPDATE SET
                task_id = excluded.task_id,
                preferred_tool = excluded.preferred_tool,
                project = excluded.project,
                status = excluded.status,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                finished_at = excluded.finished_at,
                remote_host = excluded.remote_host,
                branch_name = excluded.branch_name,
                worktree_path = excluded.worktree_path,
                pull_request_url = excluded.pull_request_url,
                follow_up_request = excluded.follow_up_request,
                summary = excluded.summary,
                notes = excluded.notes,
                error_message = excluded.error_message,
                review_request_head_oid = excluded.review_request_head_oid,
                review_request_user = excluded.review_request_user
            "#,
            dispatch_id,
            task_id,
            preferred_tool,
            project,
            status,
            created_at,
            updated_at,
            finished_at,
            remote_host,
            branch_name,
            worktree_path,
            pull_request_url,
            follow_up_request,
            summary,
            notes,
            error_message,
            review_request_head_oid,
            review_request_user,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not save the dispatch record for task {}",
            record.task_id
        ))?;

        Ok(())
    }

    pub async fn latest_dispatch_for_task(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        Ok(self.dispatches_for_task(task_id).await?.into_iter().next())
    }

    pub async fn dispatches_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let task_id = validate_single_normal_path_component(
            task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let task_id_ref = task_id.as_str();
        let rows = sqlx::query_as!(
            records::TaskDispatchRow,
            r#"
            SELECT
                dispatch_id AS "dispatch_id!",
                task_id AS "task_id!",
                preferred_tool AS "preferred_tool!",
                project AS "project!",
                status AS "status!",
                created_at AS "created_at!",
                updated_at AS "updated_at!",
                finished_at AS "finished_at?",
                remote_host AS "remote_host!",
                branch_name AS "branch_name?",
                worktree_path AS "worktree_path?",
                pull_request_url AS "pull_request_url?",
                follow_up_request AS "follow_up_request?",
                summary AS "summary?",
                notes AS "notes?",
                error_message AS "error_message?",
                review_request_head_oid AS "review_request_head_oid?",
                review_request_user AS "review_request_user?"
            FROM task_dispatches
            WHERE task_id = ?1
            ORDER BY created_at DESC
            "#,
            task_id_ref,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not load dispatch history for task {task_id}"
        ))?;

        rows.into_iter().map(task_dispatch_from_record).collect()
    }

    pub async fn latest_dispatches_for_tasks(
        &self,
        task_ids: &[String],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut records = Vec::new();
        for task_id in task_ids {
            if let Some(record) = self.latest_dispatch_for_task(task_id).await? {
                records.push(record);
            }
        }

        Ok(records)
    }

    pub async fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let limit = limit.map(|value| value as i64);
        let mut connection = self.database.connect().await?;
        let rows = if let Some(limit) = limit {
            sqlx::query_as!(
                records::TaskDispatchRow,
                r#"
                SELECT
                    dispatch_id AS "dispatch_id!",
                    task_id AS "task_id!",
                    preferred_tool AS "preferred_tool!",
                    project AS "project!",
                    status AS "status!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    finished_at AS "finished_at?",
                    remote_host AS "remote_host!",
                    branch_name AS "branch_name?",
                    worktree_path AS "worktree_path?",
                    pull_request_url AS "pull_request_url?",
                    follow_up_request AS "follow_up_request?",
                    summary AS "summary?",
                    notes AS "notes?",
                    error_message AS "error_message?",
                    review_request_head_oid AS "review_request_head_oid?",
                    review_request_user AS "review_request_user?"
                FROM task_dispatches
                ORDER BY created_at DESC
                LIMIT ?1
                "#,
                limit,
            )
            .fetch_all(&mut *connection)
            .await
        } else {
            sqlx::query_as!(
                records::TaskDispatchRow,
                r#"
                SELECT
                    dispatch_id AS "dispatch_id!",
                    task_id AS "task_id!",
                    preferred_tool AS "preferred_tool!",
                    project AS "project!",
                    status AS "status!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    finished_at AS "finished_at?",
                    remote_host AS "remote_host!",
                    branch_name AS "branch_name?",
                    worktree_path AS "worktree_path?",
                    pull_request_url AS "pull_request_url?",
                    follow_up_request AS "follow_up_request?",
                    summary AS "summary?",
                    notes AS "notes?",
                    error_message AS "error_message?",
                    review_request_head_oid AS "review_request_head_oid?",
                    review_request_user AS "review_request_user?"
                FROM task_dispatches
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(&mut *connection)
            .await
        }
        .database_error_with("Could not list dispatch records")?;

        rows.into_iter().map(task_dispatch_from_record).collect()
    }

    pub async fn task_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        let mut connection = self.database.connect().await?;
        let rows = sqlx::query_as!(
            records::TaskIdRow,
            r#"
            SELECT DISTINCT task_id AS "task_id!"
            FROM task_dispatches
            ORDER BY task_id ASC
            "#,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with("Could not load task ids with dispatch history")?;

        Ok(rows.into_iter().map(|row| row.task_id).collect())
    }

    pub async fn get_dispatch(
        &self,
        task_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        let task_id = validate_single_normal_path_component(
            task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;
        let dispatch_id = validate_single_normal_path_component(
            dispatch_id,
            "Dispatch id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let task_id_ref = task_id.as_str();
        let dispatch_id_ref = dispatch_id.as_str();
        let row = sqlx::query_as!(
            records::TaskDispatchRow,
            r#"
            SELECT
                dispatch_id AS "dispatch_id!",
                task_id AS "task_id!",
                preferred_tool AS "preferred_tool!",
                project AS "project!",
                status AS "status!",
                created_at AS "created_at!",
                updated_at AS "updated_at!",
                finished_at AS "finished_at?",
                remote_host AS "remote_host!",
                branch_name AS "branch_name?",
                worktree_path AS "worktree_path?",
                pull_request_url AS "pull_request_url?",
                follow_up_request AS "follow_up_request?",
                summary AS "summary?",
                notes AS "notes?",
                error_message AS "error_message?",
                review_request_head_oid AS "review_request_head_oid?",
                review_request_user AS "review_request_user?"
            FROM task_dispatches
            WHERE task_id = ?1 AND dispatch_id = ?2
            "#,
            task_id_ref,
            dispatch_id_ref,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not load the dispatch record {dispatch_id} for task {task_id}"
        ))?;

        row.map(task_dispatch_from_record).transpose()
    }

    pub async fn delete_dispatch_history_for_task(&self, task_id: &str) -> Result<(), TrackError> {
        let task_id = validate_single_normal_path_component(
            task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let task_id_ref = task_id.as_str();
        sqlx::query!(
            "DELETE FROM task_dispatches WHERE task_id = ?1",
            task_id_ref
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not remove the dispatch history for task {task_id}"
        ))?;

        Ok(())
    }
}

fn task_dispatch_from_record(
    record: records::TaskDispatchRow,
) -> Result<TaskDispatchRecord, TrackError> {
    let dispatch_id = record.dispatch_id;
    let created_at = parse_iso_8601_millis(&record.created_at).map_err(|error| {
        TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Dispatch {dispatch_id} has an invalid created_at timestamp: {error}"),
        )
    })?;
    let updated_at = parse_iso_8601_millis(&record.updated_at).map_err(|error| {
        TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Dispatch {dispatch_id} has an invalid updated_at timestamp: {error}"),
        )
    })?;
    let finished_at = record
        .finished_at
        .map(|value| parse_iso_8601_millis(&value))
        .transpose()
        .map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Dispatch {dispatch_id} has an invalid finished_at timestamp: {error}"),
            )
        })?;

    Ok(TaskDispatchRecord {
        dispatch_id,
        task_id: record.task_id,
        preferred_tool: parse_preferred_tool(record.preferred_tool.as_str())?,
        project: record.project,
        status: parse_dispatch_status(record.status.as_str())?,
        created_at,
        updated_at,
        finished_at,
        remote_host: record.remote_host,
        branch_name: record.branch_name,
        worktree_path: record.worktree_path,
        pull_request_url: record.pull_request_url,
        follow_up_request: record.follow_up_request,
        summary: record.summary,
        notes: record.notes,
        error_message: record.error_message,
        review_request_head_oid: record.review_request_head_oid,
        review_request_user: record.review_request_user,
    })
}

fn parse_dispatch_status(value: &str) -> Result<DispatchStatus, TrackError> {
    match value {
        "preparing" => Ok(DispatchStatus::Preparing),
        "running" => Ok(DispatchStatus::Running),
        "succeeded" => Ok(DispatchStatus::Succeeded),
        "canceled" => Ok(DispatchStatus::Canceled),
        "failed" => Ok(DispatchStatus::Failed),
        "blocked" => Ok(DispatchStatus::Blocked),
        _ => Err(TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Dispatch status `{value}` is not valid."),
        )),
    }
}

fn parse_preferred_tool(value: &str) -> Result<RemoteAgentPreferredTool, TrackError> {
    RemoteAgentPreferredTool::from_str(value).ok_or_else(|| {
        TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Remote agent preferred tool `{value}` is not valid."),
        )
    })
}

#[cfg(test)]
mod tests {
    use track_types::time_utils::{now_utc, parse_iso_8601_millis};
    use track_types::types::{
        DispatchStatus, Priority, RemoteAgentPreferredTool, Status, TaskSource,
    };

    use super::DispatchRepository;
    use crate::test_support::{sample_dispatch, sample_task, temporary_database_path};

    #[tokio::test]
    async fn create_dispatch_keeps_new_record_in_memory_until_callers_save_launch_context() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        let timestamp = now_utc();
        let task = sample_task(
            "20260403-111700-dispatch-race",
            "project-a",
            Priority::High,
            Status::Open,
            "Dispatch race regression test",
            "2026-04-03T11:17:00.000Z",
            "2026-04-03T11:17:00.000Z",
            Some(TaskSource::Web),
        );
        let task = track_types::types::Task {
            created_at: timestamp,
            updated_at: timestamp,
            ..task
        };

        let mut record = repository
            .create_dispatch(
                &task,
                "198.51.100.10",
                track_types::types::RemoteAgentPreferredTool::Codex,
            )
            .expect("dispatch record should build");

        assert!(
            repository
                .latest_dispatch_for_task(&task.id)
                .await
                .expect("dispatch lookup should succeed")
                .is_none(),
            "a newly created dispatch should stay invisible until its launch context is saved",
        );

        record.branch_name = Some(format!("track/{}", record.dispatch_id));
        record.worktree_path = Some(format!(
            "/home/track/workspace/{}/worktrees/{}",
            task.project, record.dispatch_id
        ));
        repository
            .save_dispatch(&record)
            .await
            .expect("dispatch should save with launch context");

        let saved = repository
            .latest_dispatch_for_task(&task.id)
            .await
            .expect("dispatch lookup should succeed")
            .expect("saved dispatch should be visible");

        assert_eq!(saved.dispatch_id, record.dispatch_id);
        assert_eq!(saved.branch_name, record.branch_name);
        assert_eq!(saved.worktree_path, record.worktree_path);
    }

    #[tokio::test]
    async fn save_dispatch_upserts_and_get_dispatch_returns_latest_fields() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        let original = sample_dispatch(
            "dispatch-1",
            "task-1",
            "project-a",
            RemoteAgentPreferredTool::Codex,
            DispatchStatus::Preparing,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        repository
            .save_dispatch(&original)
            .await
            .expect("original dispatch should save");

        let mut updated = sample_dispatch(
            "dispatch-1",
            "task-1",
            "project-a",
            RemoteAgentPreferredTool::Claude,
            DispatchStatus::Succeeded,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T11:00:00.000Z",
        );
        updated.finished_at =
            Some(parse_iso_8601_millis("2026-04-05T11:00:00.000Z").expect("fixture should parse"));
        updated.summary = Some("Applied fix remotely".to_owned());
        updated.pull_request_url = Some("https://github.com/acme/project-a/pull/42".to_owned());
        repository
            .save_dispatch(&updated)
            .await
            .expect("updated dispatch should save");

        let loaded = repository
            .get_dispatch("task-1", "dispatch-1")
            .await
            .expect("dispatch should load")
            .expect("dispatch should exist");
        assert_eq!(loaded, updated);
    }

    #[tokio::test]
    async fn dispatches_for_task_and_latest_dispatch_order_by_created_at_desc() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        let older = sample_dispatch(
            "dispatch-older",
            "task-1",
            "project-a",
            RemoteAgentPreferredTool::Codex,
            DispatchStatus::Failed,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T09:10:00.000Z",
        );
        let newer = sample_dispatch(
            "dispatch-newer",
            "task-1",
            "project-a",
            RemoteAgentPreferredTool::Claude,
            DispatchStatus::Running,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:05:00.000Z",
        );
        repository
            .save_dispatch(&older)
            .await
            .expect("older dispatch should save");
        repository
            .save_dispatch(&newer)
            .await
            .expect("newer dispatch should save");

        let history = repository
            .dispatches_for_task("task-1")
            .await
            .expect("dispatch history should load");
        assert_eq!(
            history
                .iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-newer", "dispatch-older"],
        );

        let latest = repository
            .latest_dispatch_for_task("task-1")
            .await
            .expect("latest dispatch should load")
            .expect("latest dispatch should exist");
        assert_eq!(latest.dispatch_id, "dispatch-newer");
    }

    #[tokio::test]
    async fn latest_dispatches_for_tasks_returns_one_latest_record_per_task() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        repository
            .save_dispatch(&sample_dispatch(
                "dispatch-a1",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Failed,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ))
            .await
            .expect("older task a dispatch should save");
        repository
            .save_dispatch(&sample_dispatch(
                "dispatch-a2",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Succeeded,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ))
            .await
            .expect("newer task a dispatch should save");
        repository
            .save_dispatch(&sample_dispatch(
                "dispatch-b1",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Running,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ))
            .await
            .expect("task b dispatch should save");

        let latest = repository
            .latest_dispatches_for_tasks(&[
                "task-a".to_owned(),
                "task-b".to_owned(),
                "missing".to_owned(),
            ])
            .await
            .expect("latest dispatches should load");

        assert_eq!(
            latest
                .iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-a2", "dispatch-b1"],
        );
    }

    #[tokio::test]
    async fn list_dispatches_honors_optional_limit() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        for record in [
            sample_dispatch(
                "dispatch-1",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Failed,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ),
            sample_dispatch(
                "dispatch-2",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ),
            sample_dispatch(
                "dispatch-3",
                "task-c",
                "project-c",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Succeeded,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ),
        ] {
            repository
                .save_dispatch(&record)
                .await
                .expect("dispatch should save");
        }

        let all = repository
            .list_dispatches(None)
            .await
            .expect("dispatch list should load");
        assert_eq!(
            all.iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-3", "dispatch-2", "dispatch-1"],
        );

        let limited = repository
            .list_dispatches(Some(2))
            .await
            .expect("limited dispatch list should load");
        assert_eq!(
            limited
                .iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-3", "dispatch-2"],
        );
    }

    #[tokio::test]
    async fn task_ids_with_history_returns_sorted_unique_ids() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        for record in [
            sample_dispatch(
                "dispatch-1",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Preparing,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ),
            sample_dispatch(
                "dispatch-2",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ),
            sample_dispatch(
                "dispatch-3",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Succeeded,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ),
        ] {
            repository
                .save_dispatch(&record)
                .await
                .expect("dispatch should save");
        }

        let task_ids = repository
            .task_ids_with_history()
            .await
            .expect("task ids should load");
        assert_eq!(task_ids, vec!["task-a".to_owned(), "task-b".to_owned()]);
    }

    #[tokio::test]
    async fn delete_dispatch_history_for_task_removes_only_target_rows() {
        let (_directory, database_path) = temporary_database_path();
        let repository = DispatchRepository::new(Some(database_path))
            .await
            .expect("repository should open");

        repository
            .save_dispatch(&sample_dispatch(
                "dispatch-a1",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Failed,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ))
            .await
            .expect("task a dispatch should save");
        repository
            .save_dispatch(&sample_dispatch(
                "dispatch-b1",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ))
            .await
            .expect("task b dispatch should save");

        repository
            .delete_dispatch_history_for_task("task-a")
            .await
            .expect("task a history should delete");

        assert!(repository
            .dispatches_for_task("task-a")
            .await
            .expect("task a history should load")
            .is_empty());
        assert_eq!(
            repository
                .dispatches_for_task("task-b")
                .await
                .expect("task b history should load")
                .len(),
            1,
        );
    }
}
