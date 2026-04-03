use std::path::PathBuf;

use sqlx::Row;
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_millis};
use track_types::types::{DispatchStatus, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord};

use crate::database::DatabaseContext;

#[derive(Debug, Clone)]
pub struct ReviewDispatchRepository {
    database: DatabaseContext,
}

impl ReviewDispatchRepository {
    pub fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize()?;

        Ok(Self { database })
    }

    pub fn create_dispatch(
        &self,
        review: &ReviewRecord,
        remote_host: &str,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Result<ReviewRunRecord, TrackError> {
        let timestamp = now_utc();
        let record = ReviewRunRecord {
            dispatch_id: format!("dispatch-{}", timestamp.unix_timestamp_nanos()),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool,
            status: DispatchStatus::Preparing,
            created_at: timestamp,
            updated_at: timestamp,
            finished_at: None,
            remote_host: remote_host.to_owned(),
            branch_name: None,
            worktree_path: None,
            follow_up_request: None,
            target_head_oid: None,
            summary: None,
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };

        self.save_dispatch(&record)?;
        Ok(record)
    }

    pub fn save_dispatch(&self, record: &ReviewRunRecord) -> Result<(), TrackError> {
        let record = record.clone();
        validate_single_normal_path_component(
            &record.review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;
        validate_single_normal_path_component(
            &record.dispatch_id,
            "Dispatch id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    r#"
                    INSERT INTO review_runs (
                        dispatch_id, review_id, pull_request_url, repository_full_name,
                        workspace_key, preferred_tool, status, created_at, updated_at,
                        finished_at, remote_host, branch_name, worktree_path,
                        follow_up_request, target_head_oid, summary, review_submitted,
                        github_review_id, github_review_url, notes, error_message
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
                    ON CONFLICT(dispatch_id) DO UPDATE SET
                        review_id = excluded.review_id,
                        pull_request_url = excluded.pull_request_url,
                        repository_full_name = excluded.repository_full_name,
                        workspace_key = excluded.workspace_key,
                        preferred_tool = excluded.preferred_tool,
                        status = excluded.status,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at,
                        finished_at = excluded.finished_at,
                        remote_host = excluded.remote_host,
                        branch_name = excluded.branch_name,
                        worktree_path = excluded.worktree_path,
                        follow_up_request = excluded.follow_up_request,
                        target_head_oid = excluded.target_head_oid,
                        summary = excluded.summary,
                        review_submitted = excluded.review_submitted,
                        github_review_id = excluded.github_review_id,
                        github_review_url = excluded.github_review_url,
                        notes = excluded.notes,
                        error_message = excluded.error_message
                    "#,
                )
                .bind(&record.dispatch_id)
                .bind(&record.review_id)
                .bind(&record.pull_request_url)
                .bind(&record.repository_full_name)
                .bind(&record.workspace_key)
                .bind(record.preferred_tool.as_str())
                .bind(record.status.as_str())
                .bind(format_iso_8601_millis(record.created_at))
                .bind(format_iso_8601_millis(record.updated_at))
                .bind(record.finished_at.map(format_iso_8601_millis))
                .bind(&record.remote_host)
                .bind(record.branch_name.as_deref())
                .bind(record.worktree_path.as_deref())
                .bind(record.follow_up_request.as_deref())
                .bind(record.target_head_oid.as_deref())
                .bind(record.summary.as_deref())
                .bind(record.review_submitted as i64)
                .bind(record.github_review_id.as_deref())
                .bind(record.github_review_url.as_deref())
                .bind(record.notes.as_deref())
                .bind(record.error_message.as_deref())
                .execute(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not save the review run record for review {}: {error}",
                            record.review_id
                        ),
                    )
                })?;

                Ok(())
            })
        })
    }

    pub fn latest_dispatch_for_review(
        &self,
        review_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        Ok(self.dispatches_for_review(review_id)?.into_iter().next())
    }

    pub fn dispatches_for_review(
        &self,
        review_id: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT *
                    FROM review_runs
                    WHERE review_id = ?1
                    ORDER BY created_at DESC
                    "#,
                )
                .bind(&review_id)
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not load review runs for {review_id}: {error}"),
                    )
                })?;

                rows.into_iter().map(review_run_from_row).collect()
            })
        })
    }

    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let limit = limit.map(|value| value as i64);
        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = if let Some(limit) = limit {
                    sqlx::query(
                        r#"
                        SELECT *
                        FROM review_runs
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
                        FROM review_runs
                        ORDER BY created_at DESC
                        "#,
                    )
                    .fetch_all(&mut *connection)
                    .await
                }
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not list review run records: {error}"),
                    )
                })?;

                rows.into_iter().map(review_run_from_row).collect()
            })
        })
    }

    pub fn review_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT DISTINCT review_id
                    FROM review_runs
                    ORDER BY review_id ASC
                    "#,
                )
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!("Could not load review ids with run history: {error}"),
                    )
                })?;

                Ok(rows
                    .into_iter()
                    .map(|row| row.get::<String, _>("review_id"))
                    .collect())
            })
        })
    }

    pub fn get_dispatch(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
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
                    FROM review_runs
                    WHERE review_id = ?1 AND dispatch_id = ?2
                    "#,
                )
                .bind(&review_id)
                .bind(&dispatch_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not load the review run {dispatch_id} for review {review_id}: {error}"
                        ),
                    )
                })?;

                row.map(review_run_from_row).transpose()
            })
        })
    }

    pub fn delete_dispatch_history_for_review(&self, review_id: &str) -> Result<(), TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM review_runs WHERE review_id = ?1")
                    .bind(&review_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::DispatchWriteFailed,
                            format!(
                                "Could not remove the review dispatch history for {review_id}: {error}"
                            ),
                        )
                    })?;

                Ok(())
            })
        })
    }
}

fn review_run_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ReviewRunRecord, TrackError> {
    let dispatch_id = row.get::<String, _>("dispatch_id");
    let created_at =
        parse_iso_8601_millis(&row.get::<String, _>("created_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Review run {dispatch_id} has an invalid created_at timestamp: {error}"),
            )
        })?;
    let updated_at =
        parse_iso_8601_millis(&row.get::<String, _>("updated_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Review run {dispatch_id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
    let finished_at = row
        .get::<Option<String>, _>("finished_at")
        .map(|value| parse_iso_8601_millis(&value))
        .transpose()
        .map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Review run {dispatch_id} has an invalid finished_at timestamp: {error}"),
            )
        })?;

    Ok(ReviewRunRecord {
        dispatch_id,
        review_id: row.get::<String, _>("review_id"),
        pull_request_url: row.get::<String, _>("pull_request_url"),
        repository_full_name: row.get::<String, _>("repository_full_name"),
        workspace_key: row.get::<String, _>("workspace_key"),
        preferred_tool: parse_preferred_tool(
            row.try_get::<String, _>("preferred_tool")
                .unwrap_or_else(|_| "codex".to_owned())
                .as_str(),
        )?,
        status: parse_dispatch_status(row.get::<String, _>("status").as_str())?,
        created_at,
        updated_at,
        finished_at,
        remote_host: row.get::<String, _>("remote_host"),
        branch_name: row.get::<Option<String>, _>("branch_name"),
        worktree_path: row.get::<Option<String>, _>("worktree_path"),
        follow_up_request: row.get::<Option<String>, _>("follow_up_request"),
        target_head_oid: row.get::<Option<String>, _>("target_head_oid"),
        summary: row.get::<Option<String>, _>("summary"),
        review_submitted: row.get::<i64, _>("review_submitted") != 0,
        github_review_id: row.get::<Option<String>, _>("github_review_id"),
        github_review_url: row.get::<Option<String>, _>("github_review_url"),
        notes: row.get::<Option<String>, _>("notes"),
        error_message: row.get::<Option<String>, _>("error_message"),
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
