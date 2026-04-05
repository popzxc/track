use std::path::PathBuf;

use sqlx::Row;
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, parse_iso_8601_millis};
use track_types::types::{RemoteAgentPreferredTool, ReviewRecord};

use crate::database::{resolve_database_path, DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone)]
pub struct ReviewRepository {
    database: DatabaseContext,
    reviews_dir: PathBuf,
}

impl ReviewRepository {
    pub async fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let reviews_dir = resolve_database_path(database_path)?;
        let database = DatabaseContext::new(Some(reviews_dir.clone())).await?;
        database.initialize().await?;

        Ok(Self {
            database,
            reviews_dir,
        })
    }

    pub fn reviews_dir(&self) -> &std::path::Path {
        &self.reviews_dir
    }

    pub async fn save_review(&self, review: &ReviewRecord) -> Result<(), TrackError> {
        let review = review.clone();
        let mut connection = self.database.connect().await?;
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
        .database_error_with(format!("Could not save review {}", review.id))?;

        Ok(())
    }

    pub async fn list_reviews(&self) -> Result<Vec<ReviewRecord>, TrackError> {
        let mut connection = self.database.connect().await?;
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
        .database_error_with("Could not list reviews from SQLite")?;

        rows.into_iter().map(review_from_row).collect()
    }

    pub async fn get_review(&self, id: &str) -> Result<ReviewRecord, TrackError> {
        let review_id = validate_single_normal_path_component(
            id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
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
        .database_error_with(format!("Could not load review {review_id}"))?
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::TaskNotFound,
                format!("Review {review_id} was not found."),
            )
        })?;

        review_from_row(row)
    }

    pub async fn delete_review(&self, id: &str) -> Result<(), TrackError> {
        let review_id = validate_single_normal_path_component(
            id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        sqlx::query("DELETE FROM reviews WHERE id = ?1")
            .bind(&review_id)
            .execute(&mut *connection)
            .await
            .database_error_with(format!("Could not delete review {review_id}"))?;

        Ok(())
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

#[cfg(test)]
mod tests {
    use track_types::errors::ErrorCode;
    use track_types::types::RemoteAgentPreferredTool;

    use super::ReviewRepository;
    use crate::test_support::{sample_review, temporary_database_path};

    #[tokio::test]
    async fn save_review_upserts_and_get_review_returns_latest_fields() {
        let (_directory, database_path) = temporary_database_path();
        let repository = ReviewRepository::new(Some(database_path))
            .await
            .expect("review repository should resolve");

        let original = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        repository
            .save_review(&original)
            .await
            .expect("review should save");

        let mut updated = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T11:00:00.000Z",
        );
        updated.pull_request_title = "Updated review title".to_owned();
        updated.project = None;
        repository
            .save_review(&updated)
            .await
            .expect("updated review should save");

        let loaded = repository
            .get_review("review-42")
            .await
            .expect("review should load");
        assert_eq!(loaded, updated);
    }

    #[tokio::test]
    async fn list_reviews_orders_by_updated_at_desc() {
        let (_directory, database_path) = temporary_database_path();
        let repository = ReviewRepository::new(Some(database_path))
            .await
            .expect("review repository should resolve");

        let older = sample_review(
            "review-41",
            41,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        let newer = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T10:30:00.000Z",
            "2026-04-05T11:30:00.000Z",
        );
        repository
            .save_review(&older)
            .await
            .expect("older review should save");
        repository
            .save_review(&newer)
            .await
            .expect("newer review should save");

        let reviews = repository
            .list_reviews()
            .await
            .expect("review list should load");
        assert_eq!(
            reviews
                .iter()
                .map(|review| review.id.as_str())
                .collect::<Vec<_>>(),
            vec!["review-42", "review-41"],
        );
    }

    #[tokio::test]
    async fn delete_review_removes_saved_row() {
        let (_directory, database_path) = temporary_database_path();
        let repository = ReviewRepository::new(Some(database_path))
            .await
            .expect("review repository should resolve");

        let review = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        repository
            .save_review(&review)
            .await
            .expect("review should save");

        repository
            .delete_review(&review.id)
            .await
            .expect("review should delete");

        let error = repository
            .get_review(&review.id)
            .await
            .expect_err("deleted review should be missing");
        assert_eq!(error.code, ErrorCode::TaskNotFound);
    }
}
