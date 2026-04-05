mod records;

use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::ReviewId;
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::ReviewRecord;

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct ReviewRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> ReviewRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    pub async fn save_review(&self, review: &ReviewRecord) -> Result<(), TrackError> {
        let review = review.clone();
        let mut connection = self.database.connect().await?;
        let id = review.id.as_str();
        let pull_request_url = review.pull_request_url.as_str();
        let pull_request_title = review.pull_request_title.as_str();
        let repository_full_name = review.repository_full_name.as_str();
        let repo_url = review.repo_url.as_str();
        let git_url = review.git_url.as_str();
        let base_branch = review.base_branch.as_str();
        let workspace_key = review.workspace_key.as_str();
        let preferred_tool = review.preferred_tool.as_str();
        let project = review.project.as_ref().map(|project| project.as_str());
        let main_user = review.main_user.as_str();
        let default_review_prompt = review.default_review_prompt.as_deref();
        let extra_instructions = review.extra_instructions.as_deref();
        let pull_request_number = review.pull_request_number as i64;
        let created_at = format_iso_8601_millis(review.created_at);
        let updated_at = format_iso_8601_millis(review.updated_at);
        sqlx::query!(
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
            id,
            pull_request_url,
            pull_request_number,
            pull_request_title,
            repository_full_name,
            repo_url,
            git_url,
            base_branch,
            workspace_key,
            preferred_tool,
            project,
            main_user,
            default_review_prompt,
            extra_instructions,
            created_at,
            updated_at,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not save review {}", review.id))?;

        Ok(())
    }

    pub async fn list_reviews(&self) -> Result<Vec<ReviewRecord>, TrackError> {
        let mut connection = self.database.connect().await?;
        let rows = sqlx::query_as!(
            records::ReviewRow,
            r#"
            SELECT
                id AS "id!",
                pull_request_url AS "pull_request_url!",
                pull_request_number AS "pull_request_number!",
                pull_request_title AS "pull_request_title!",
                repository_full_name AS "repository_full_name!",
                repo_url AS "repo_url!",
                git_url AS "git_url!",
                base_branch AS "base_branch!",
                workspace_key AS "workspace_key!",
                preferred_tool AS "preferred_tool!",
                project AS "project?",
                main_user AS "main_user!",
                default_review_prompt AS "default_review_prompt?",
                extra_instructions AS "extra_instructions?",
                created_at AS "created_at!",
                updated_at AS "updated_at!"
            FROM reviews
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with("Could not list reviews from SQLite")?;

        rows.into_iter().map(ReviewRecord::try_from).collect()
    }

    pub async fn get_review(&self, review_id: &ReviewId) -> Result<ReviewRecord, TrackError> {
        let mut connection = self.database.connect().await?;
        let review_id_ref = review_id.as_str();
        let row = sqlx::query_as!(
            records::ReviewRow,
            r#"
            SELECT
                id AS "id!",
                pull_request_url AS "pull_request_url!",
                pull_request_number AS "pull_request_number!",
                pull_request_title AS "pull_request_title!",
                repository_full_name AS "repository_full_name!",
                repo_url AS "repo_url!",
                git_url AS "git_url!",
                base_branch AS "base_branch!",
                workspace_key AS "workspace_key!",
                preferred_tool AS "preferred_tool!",
                project AS "project?",
                main_user AS "main_user!",
                default_review_prompt AS "default_review_prompt?",
                extra_instructions AS "extra_instructions?",
                created_at AS "created_at!",
                updated_at AS "updated_at!"
            FROM reviews
            WHERE id = ?1
            "#,
            review_id_ref,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!("Could not load review {review_id}"))?
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::TaskNotFound,
                format!("Review {review_id} was not found."),
            )
        })?;

        ReviewRecord::try_from(row)
    }

    pub async fn delete_review(&self, review_id: &ReviewId) -> Result<(), TrackError> {
        let mut connection = self.database.connect().await?;
        let review_id_ref = review_id.as_str();
        sqlx::query!("DELETE FROM reviews WHERE id = ?1", review_id_ref)
            .execute(&mut *connection)
            .await
            .database_error_with(format!("Could not delete review {review_id}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use track_types::errors::ErrorCode;
    use track_types::types::RemoteAgentPreferredTool;

    use crate::database::DatabaseContext;
    use crate::test_support::{parse_review_id, sample_review, temporary_database_path};

    #[tokio::test]
    async fn save_review_upserts_and_get_review_returns_latest_fields() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.review_repository();

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
            .get_review(&parse_review_id("review-42"))
            .await
            .expect("review should load");
        assert_eq!(loaded, updated);
    }

    #[tokio::test]
    async fn list_reviews_orders_by_updated_at_desc() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.review_repository();

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
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.review_repository();

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
