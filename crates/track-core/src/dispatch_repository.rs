use std::path::PathBuf;

use sqlx::Row;

use crate::database::DatabaseContext;
use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;
use crate::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_millis};
use crate::types::{DispatchStatus, RemoteAgentPreferredTool, Task, TaskDispatchRecord};

#[derive(Debug, Clone)]
pub struct DispatchRepository {
    database: DatabaseContext,
}

impl DispatchRepository {
    pub fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize()?;

        Ok(Self { database })
    }

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

        self.save_dispatch(&record)?;
        Ok(record)
    }

    pub fn save_dispatch(&self, record: &TaskDispatchRecord) -> Result<(), TrackError> {
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

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query(
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
                )
                .bind(&record.dispatch_id)
                .bind(&record.task_id)
                .bind(record.preferred_tool.as_str())
                .bind(&record.project)
                .bind(record.status.as_str())
                .bind(format_iso_8601_millis(record.created_at))
                .bind(format_iso_8601_millis(record.updated_at))
                .bind(record.finished_at.map(format_iso_8601_millis))
                .bind(&record.remote_host)
                .bind(record.branch_name.as_deref())
                .bind(record.worktree_path.as_deref())
                .bind(record.pull_request_url.as_deref())
                .bind(record.follow_up_request.as_deref())
                .bind(record.summary.as_deref())
                .bind(record.notes.as_deref())
                .bind(record.error_message.as_deref())
                .bind(record.review_request_head_oid.as_deref())
                .bind(record.review_request_user.as_deref())
                .execute(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not save the dispatch record for task {}: {error}",
                            record.task_id
                        ),
                    )
                })?;

                Ok(())
            })
        })
    }

    pub fn latest_dispatch_for_task(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        Ok(self.dispatches_for_task(task_id)?.into_iter().next())
    }

    pub fn dispatches_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let task_id = validate_single_normal_path_component(
            task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT *
                    FROM task_dispatches
                    WHERE task_id = ?1
                    ORDER BY created_at DESC
                    "#,
                )
                .bind(&task_id)
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not load dispatch history for task {task_id}: {error}"),
                    )
                })?;

                rows.into_iter().map(task_dispatch_from_row).collect()
            })
        })
    }

    pub fn latest_dispatches_for_tasks(
        &self,
        task_ids: &[String],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut records = Vec::new();
        for task_id in task_ids {
            if let Some(record) = self.latest_dispatch_for_task(task_id)? {
                records.push(record);
            }
        }

        Ok(records)
    }

    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let limit = limit.map(|value| value as i64);
        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = if let Some(limit) = limit {
                    sqlx::query(
                        r#"
                        SELECT *
                        FROM task_dispatches
                        ORDER BY created_at DESC
                        LIMIT ?1
                        "#,
                    )
                    .bind(limit)
                    .fetch_all(&mut *connection)
                    .await
                } else {
                    sqlx::query(
                        r#"
                        SELECT *
                        FROM task_dispatches
                        ORDER BY created_at DESC
                        "#,
                    )
                    .fetch_all(&mut *connection)
                    .await
                }
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not list dispatch records: {error}"),
                    )
                })?;

                rows.into_iter().map(task_dispatch_from_row).collect()
            })
        })
    }

    pub fn task_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT DISTINCT task_id
                    FROM task_dispatches
                    ORDER BY task_id ASC
                    "#,
                )
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not load task ids with dispatch history: {error}"),
                    )
                })?;

                Ok(rows
                    .into_iter()
                    .map(|row| row.get::<String, _>("task_id"))
                    .collect())
            })
        })
    }

    pub fn get_dispatch(
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

        self.database.run(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    r#"
                    SELECT *
                    FROM task_dispatches
                    WHERE task_id = ?1 AND dispatch_id = ?2
                    "#,
                )
                .bind(&task_id)
                .bind(&dispatch_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not load the dispatch record {dispatch_id} for task {task_id}: {error}"
                        ),
                    )
                })?;

                row.map(task_dispatch_from_row).transpose()
            })
        })
    }

    pub fn delete_dispatch_history_for_task(&self, task_id: &str) -> Result<(), TrackError> {
        let task_id = validate_single_normal_path_component(
            task_id,
            "Task id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM task_dispatches WHERE task_id = ?1")
                    .bind(&task_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::DispatchWriteFailed,
                            format!(
                                "Could not remove the dispatch history for task {task_id}: {error}"
                            ),
                        )
                    })?;

                Ok(())
            })
        })
    }
}

fn task_dispatch_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TaskDispatchRecord, TrackError> {
    let dispatch_id = row.get::<String, _>("dispatch_id");
    let created_at =
        parse_iso_8601_millis(&row.get::<String, _>("created_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Dispatch {dispatch_id} has an invalid created_at timestamp: {error}"),
            )
        })?;
    let updated_at =
        parse_iso_8601_millis(&row.get::<String, _>("updated_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Dispatch {dispatch_id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
    let finished_at = row
        .get::<Option<String>, _>("finished_at")
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
        task_id: row.get::<String, _>("task_id"),
        preferred_tool: parse_preferred_tool(
            row.try_get::<String, _>("preferred_tool")
                .unwrap_or_else(|_| "codex".to_owned())
                .as_str(),
        )?,
        project: row.get::<String, _>("project"),
        status: parse_dispatch_status(row.get::<String, _>("status").as_str())?,
        created_at,
        updated_at,
        finished_at,
        remote_host: row.get::<String, _>("remote_host"),
        branch_name: row.get::<Option<String>, _>("branch_name"),
        worktree_path: row.get::<Option<String>, _>("worktree_path"),
        pull_request_url: row.get::<Option<String>, _>("pull_request_url"),
        follow_up_request: row.get::<Option<String>, _>("follow_up_request"),
        summary: row.get::<Option<String>, _>("summary"),
        notes: row.get::<Option<String>, _>("notes"),
        error_message: row.get::<Option<String>, _>("error_message"),
        review_request_head_oid: row.get::<Option<String>, _>("review_request_head_oid"),
        review_request_user: row.get::<Option<String>, _>("review_request_user"),
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
