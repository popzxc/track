use std::path::PathBuf;

use sqlx::Row;
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, parse_iso_8601_millis};
use track_types::types::{RemoteAgentPreferredTool, ReviewRecord};

use crate::database::DatabaseContext;

#[derive(Debug, Clone)]
pub struct ReviewRepository {
    database: DatabaseContext,
}

impl ReviewRepository {
    pub async fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize().await?;

        Ok(Self { database })
    }

    pub fn reviews_dir(&self) -> &std::path::Path {
        self.database.database_path()
    }

    pub async fn save_review(&self, review: &ReviewRecord) -> Result<(), TrackError> {
        let review = review.clone();
        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        r#"
                    INSERT INTO reviews (
                        id, pull_request_url, pull_request_number, pull_request_title,
                        repository_full_name, repo_url, git_url, base_branch, workspace_key,
                        preferred_tool, project, main_user, default_review_prompt,
                        extra_instructions, created_at, updated_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                    ON CONFLICT(id) DO UPDATE SET
                        pull_request_url = excluded.pull_request_url,
                        pull_request_number = excluded.pull_request_number,
                        pull_request_title = excluded.pull_request_title,
                        repository_full_name = excluded.repository_full_name,
                        repo_url = excluded.repo_url,
                        git_url = excluded.git_url,
                        base_branch = excluded.base_branch,
                        workspace_key = excluded.workspace_key,
                        preferred_tool = excluded.preferred_tool,
                        project = excluded.project,
                        main_user = excluded.main_user,
                        default_review_prompt = excluded.default_review_prompt,
                        extra_instructions = excluded.extra_instructions,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at
                    "#,
                    )
                    .bind(&review.id)
                    .bind(&review.pull_request_url)
                    .bind(review.pull_request_number as i64)
                    .bind(&review.pull_request_title)
                    .bind(&review.repository_full_name)
                    .bind(&review.repo_url)
                    .bind(&review.git_url)
                    .bind(&review.base_branch)
                    .bind(&review.workspace_key)
                    .bind(review.preferred_tool.as_str())
                    .bind(review.project.as_deref())
                    .bind(&review.main_user)
                    .bind(review.default_review_prompt.as_deref())
                    .bind(review.extra_instructions.as_deref())
                    .bind(format_iso_8601_millis(review.created_at))
                    .bind(format_iso_8601_millis(review.updated_at))
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not save review {}: {error}", review.id),
                        )
                    })?;

                    Ok(())
                })
            })
            .await
    }

    pub async fn list_reviews(&self) -> Result<Vec<ReviewRecord>, TrackError> {
        self.database
            .run(move |connection| {
                Box::pin(async move {
                    let rows = sqlx::query(
                        r#"
                    SELECT
                        id, pull_request_url, pull_request_number, pull_request_title,
                        repository_full_name, repo_url, git_url, base_branch, workspace_key,
                        preferred_tool, project, main_user, default_review_prompt,
                        extra_instructions, created_at, updated_at
                    FROM reviews
                    ORDER BY updated_at DESC
                    "#,
                    )
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not list reviews from SQLite: {error}"),
                        )
                    })?;

                    rows.into_iter().map(review_from_row).collect()
                })
            })
            .await
    }

    pub async fn get_review(&self, id: &str) -> Result<ReviewRecord, TrackError> {
        let review_id = validate_single_normal_path_component(
            id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database
            .run(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        r#"
                    SELECT
                        id, pull_request_url, pull_request_number, pull_request_title,
                        repository_full_name, repo_url, git_url, base_branch, workspace_key,
                        preferred_tool, project, main_user, default_review_prompt,
                        extra_instructions, created_at, updated_at
                    FROM reviews
                    WHERE id = ?1
                    "#,
                    )
                    .bind(&review_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not load review {review_id}: {error}"),
                        )
                    })?
                    .ok_or_else(|| {
                        TrackError::new(
                            ErrorCode::TaskNotFound,
                            format!("Review {review_id} was not found."),
                        )
                    })?;

                    review_from_row(row)
                })
            })
            .await
    }

    pub async fn delete_review(&self, id: &str) -> Result<(), TrackError> {
        let review_id = validate_single_normal_path_component(
            id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM reviews WHERE id = ?1")
                        .bind(&review_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(|error| {
                            TrackError::new(
                                ErrorCode::TaskWriteFailed,
                                format!("Could not delete review {review_id}: {error}"),
                            )
                        })?;

                    Ok(())
                })
            })
            .await
    }
}

fn review_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ReviewRecord, TrackError> {
    let id = row.get::<String, _>("id");
    let created_at =
        parse_iso_8601_millis(&row.get::<String, _>("created_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Review {id} has an invalid created_at timestamp: {error}"),
            )
        })?;
    let updated_at =
        parse_iso_8601_millis(&row.get::<String, _>("updated_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Review {id} has an invalid updated_at timestamp: {error}"),
            )
        })?;

    Ok(ReviewRecord {
        id,
        pull_request_url: row.get::<String, _>("pull_request_url"),
        pull_request_number: row.get::<i64, _>("pull_request_number") as u64,
        pull_request_title: row.get::<String, _>("pull_request_title"),
        repository_full_name: row.get::<String, _>("repository_full_name"),
        repo_url: row.get::<String, _>("repo_url"),
        git_url: row.get::<String, _>("git_url"),
        base_branch: row.get::<String, _>("base_branch"),
        workspace_key: row.get::<String, _>("workspace_key"),
        preferred_tool: parse_preferred_tool(
            row.try_get::<String, _>("preferred_tool")
                .unwrap_or_else(|_| "codex".to_owned())
                .as_str(),
        )?,
        project: row.get::<Option<String>, _>("project"),
        main_user: row.get::<String, _>("main_user"),
        default_review_prompt: row.get::<Option<String>, _>("default_review_prompt"),
        extra_instructions: row.get::<Option<String>, _>("extra_instructions"),
        created_at,
        updated_at,
    })
}

fn parse_preferred_tool(value: &str) -> Result<RemoteAgentPreferredTool, TrackError> {
    RemoteAgentPreferredTool::from_str(value).ok_or_else(|| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Remote agent preferred tool `{value}` is not valid."),
        )
    })
}
