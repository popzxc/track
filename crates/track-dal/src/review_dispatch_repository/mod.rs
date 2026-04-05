mod records;

use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{DispatchStatus, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord};

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct ReviewDispatchRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> ReviewDispatchRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_dispatch(
        &self,
        review: &ReviewRecord,
        dispatch_id: &str,
        remote_host: &str,
        preferred_tool: RemoteAgentPreferredTool,
        branch_name: &str,
        worktree_path: &str,
        follow_up_request: Option<&str>,
        target_head_oid: Option<&str>,
        summary: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let timestamp = now_utc();
        let record = ReviewRunRecord {
            dispatch_id: dispatch_id.to_owned(),
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
            branch_name: Some(branch_name.to_owned()),
            worktree_path: Some(worktree_path.to_owned()),
            follow_up_request: follow_up_request.map(ToOwned::to_owned),
            target_head_oid: target_head_oid.map(ToOwned::to_owned),
            summary: summary.map(ToOwned::to_owned),
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };

        // Review runs now arrive with their queue-time context already
        // assembled, so we can persist the exact launchable record in one
        // write instead of manufacturing an incomplete row first.
        self.save_dispatch(&record).await?;

        Ok(record)
    }

    pub async fn save_dispatch(&self, record: &ReviewRunRecord) -> Result<(), TrackError> {
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

        let mut connection = self.database.connect().await?;
        let dispatch_id = record.dispatch_id.as_str();
        let review_id = record.review_id.as_str();
        let pull_request_url = record.pull_request_url.as_str();
        let repository_full_name = record.repository_full_name.as_str();
        let workspace_key = record.workspace_key.as_str();
        let preferred_tool = record.preferred_tool.as_str();
        let status = record.status.as_str();
        let created_at = format_iso_8601_millis(record.created_at);
        let updated_at = format_iso_8601_millis(record.updated_at);
        let finished_at = record.finished_at.map(format_iso_8601_millis);
        let remote_host = record.remote_host.as_str();
        let branch_name = record.branch_name.as_deref();
        let worktree_path = record.worktree_path.as_deref();
        let follow_up_request = record.follow_up_request.as_deref();
        let target_head_oid = record.target_head_oid.as_deref();
        let summary = record.summary.as_deref();
        let review_submitted = record.review_submitted as i64;
        let github_review_id = record.github_review_id.as_deref();
        let github_review_url = record.github_review_url.as_deref();
        let notes = record.notes.as_deref();
        let error_message = record.error_message.as_deref();
        sqlx::query!(
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
            dispatch_id,
            review_id,
            pull_request_url,
            repository_full_name,
            workspace_key,
            preferred_tool,
            status,
            created_at,
            updated_at,
            finished_at,
            remote_host,
            branch_name,
            worktree_path,
            follow_up_request,
            target_head_oid,
            summary,
            review_submitted,
            github_review_id,
            github_review_url,
            notes,
            error_message,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not save the review run record for review {}",
            record.review_id
        ))?;

        Ok(())
    }

    pub async fn latest_dispatch_for_review(
        &self,
        review_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        Ok(self
            .dispatches_for_review(review_id)
            .await?
            .into_iter()
            .next())
    }

    pub async fn dispatches_for_review(
        &self,
        review_id: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let review_id_ref = review_id.as_str();
        let rows = sqlx::query_as!(
            records::ReviewRunRow,
            r#"
            SELECT
                dispatch_id AS "dispatch_id!",
                review_id AS "review_id!",
                pull_request_url AS "pull_request_url!",
                repository_full_name AS "repository_full_name!",
                workspace_key AS "workspace_key!",
                preferred_tool AS "preferred_tool!",
                status AS "status!",
                created_at AS "created_at!",
                updated_at AS "updated_at!",
                finished_at AS "finished_at?",
                remote_host AS "remote_host!",
                branch_name AS "branch_name?",
                worktree_path AS "worktree_path?",
                follow_up_request AS "follow_up_request?",
                target_head_oid AS "target_head_oid?",
                summary AS "summary?",
                review_submitted AS "review_submitted!",
                github_review_id AS "github_review_id?",
                github_review_url AS "github_review_url?",
                notes AS "notes?",
                error_message AS "error_message?"
            FROM review_runs
            WHERE review_id = ?1
            ORDER BY created_at DESC
            "#,
            review_id_ref,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with(format!("Could not load review runs for {review_id}"))?;

        rows.into_iter().map(ReviewRunRecord::try_from).collect()
    }

    pub async fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let limit = limit.map(|value| value as i64);
        let mut connection = self.database.connect().await?;
        let rows = if let Some(limit) = limit {
            sqlx::query_as!(
                records::ReviewRunRow,
                r#"
                SELECT
                    dispatch_id AS "dispatch_id!",
                    review_id AS "review_id!",
                    pull_request_url AS "pull_request_url!",
                    repository_full_name AS "repository_full_name!",
                    workspace_key AS "workspace_key!",
                    preferred_tool AS "preferred_tool!",
                    status AS "status!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    finished_at AS "finished_at?",
                    remote_host AS "remote_host!",
                    branch_name AS "branch_name?",
                    worktree_path AS "worktree_path?",
                    follow_up_request AS "follow_up_request?",
                    target_head_oid AS "target_head_oid?",
                    summary AS "summary?",
                    review_submitted AS "review_submitted!",
                    github_review_id AS "github_review_id?",
                    github_review_url AS "github_review_url?",
                    notes AS "notes?",
                    error_message AS "error_message?"
                FROM review_runs
                ORDER BY created_at DESC
                LIMIT ?1
                "#,
                limit,
            )
            .fetch_all(&mut *connection)
            .await
        } else {
            sqlx::query_as!(
                records::ReviewRunRow,
                r#"
                SELECT
                    dispatch_id AS "dispatch_id!",
                    review_id AS "review_id!",
                    pull_request_url AS "pull_request_url!",
                    repository_full_name AS "repository_full_name!",
                    workspace_key AS "workspace_key!",
                    preferred_tool AS "preferred_tool!",
                    status AS "status!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    finished_at AS "finished_at?",
                    remote_host AS "remote_host!",
                    branch_name AS "branch_name?",
                    worktree_path AS "worktree_path?",
                    follow_up_request AS "follow_up_request?",
                    target_head_oid AS "target_head_oid?",
                    summary AS "summary?",
                    review_submitted AS "review_submitted!",
                    github_review_id AS "github_review_id?",
                    github_review_url AS "github_review_url?",
                    notes AS "notes?",
                    error_message AS "error_message?"
                FROM review_runs
                ORDER BY created_at DESC
                "#,
            )
            .fetch_all(&mut *connection)
            .await
        }
        .database_error_with("Could not list review run records")?;

        rows.into_iter().map(ReviewRunRecord::try_from).collect()
    }

    pub async fn review_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        let mut connection = self.database.connect().await?;
        let rows = sqlx::query_as!(
            records::ReviewIdRow,
            r#"
            SELECT DISTINCT review_id AS "review_id!"
            FROM review_runs
            ORDER BY review_id ASC
            "#,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with("Could not load review ids with run history")?;

        Ok(rows.into_iter().map(|row| row.review_id).collect())
    }

    pub async fn get_dispatch(
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

        let mut connection = self.database.connect().await?;
        let review_id_ref = review_id.as_str();
        let dispatch_id_ref = dispatch_id.as_str();
        let row = sqlx::query_as!(
            records::ReviewRunRow,
            r#"
            SELECT
                dispatch_id AS "dispatch_id!",
                review_id AS "review_id!",
                pull_request_url AS "pull_request_url!",
                repository_full_name AS "repository_full_name!",
                workspace_key AS "workspace_key!",
                preferred_tool AS "preferred_tool!",
                status AS "status!",
                created_at AS "created_at!",
                updated_at AS "updated_at!",
                finished_at AS "finished_at?",
                remote_host AS "remote_host!",
                branch_name AS "branch_name?",
                worktree_path AS "worktree_path?",
                follow_up_request AS "follow_up_request?",
                target_head_oid AS "target_head_oid?",
                summary AS "summary?",
                review_submitted AS "review_submitted!",
                github_review_id AS "github_review_id?",
                github_review_url AS "github_review_url?",
                notes AS "notes?",
                error_message AS "error_message?"
            FROM review_runs
            WHERE review_id = ?1 AND dispatch_id = ?2
            "#,
            review_id_ref,
            dispatch_id_ref,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not load the review run {dispatch_id} for review {review_id}"
        ))?;

        row.map(ReviewRunRecord::try_from).transpose()
    }

    pub async fn delete_dispatch_history_for_review(
        &self,
        review_id: &str,
    ) -> Result<(), TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        let mut connection = self.database.connect().await?;
        let review_id_ref = review_id.as_str();
        sqlx::query!(
            "DELETE FROM review_runs WHERE review_id = ?1",
            review_id_ref
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not remove the review dispatch history for {review_id}"
        ))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use track_types::time_utils::now_utc;
    use track_types::types::{DispatchStatus, RemoteAgentPreferredTool};

    use crate::database::DatabaseContext;
    use crate::test_support::{sample_review, sample_review_run, temporary_database_path};

    #[tokio::test]
    async fn create_dispatch_persists_queued_review_run_with_launch_context() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let timestamp = now_utc();
        let review = track_types::types::ReviewRecord {
            created_at: timestamp,
            updated_at: timestamp,
            ..sample_review(
                "20260403-111900-review-race",
                42,
                RemoteAgentPreferredTool::Codex,
                "2026-04-03T11:19:00.000Z",
                "2026-04-03T11:19:00.000Z",
            )
        };
        review_repository
            .save_review(&review)
            .await
            .expect("review should save");

        let record = repository
            .create_dispatch(
                &review,
                "dispatch-review-race-test",
                "198.51.100.10",
                RemoteAgentPreferredTool::Codex,
                "track-review/dispatch-review-race-test",
                "/home/track/workspace/octo-tools/review-worktrees/dispatch-review-race-test",
                None,
                None,
                None,
            )
            .await
            .expect("review run should save with launch context");

        let saved = repository
            .latest_dispatch_for_review(&review.id)
            .await
            .expect("review dispatch lookup should succeed")
            .expect("queued review run should be visible immediately");

        assert_eq!(saved.dispatch_id, record.dispatch_id);
        assert_eq!(saved.status, DispatchStatus::Preparing);
        assert_eq!(saved.branch_name, record.branch_name);
        assert_eq!(saved.worktree_path, record.worktree_path);
    }

    #[tokio::test]
    async fn save_dispatch_upserts_and_get_dispatch_returns_latest_fields() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let review = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        review_repository
            .save_review(&review)
            .await
            .expect("review should save");

        let original = sample_review_run(
            "dispatch-1",
            &review,
            RemoteAgentPreferredTool::Codex,
            DispatchStatus::Preparing,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        repository
            .save_dispatch(&original)
            .await
            .expect("original review run should save");

        let mut updated = sample_review_run(
            "dispatch-1",
            &review,
            RemoteAgentPreferredTool::Claude,
            DispatchStatus::Succeeded,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T11:00:00.000Z",
        );
        updated.review_submitted = true;
        updated.github_review_id = Some("12345".to_owned());
        updated.github_review_url =
            Some("https://github.com/acme/project-a/pull/42#pullrequestreview-12345".to_owned());
        updated.summary = Some("Submitted the review".to_owned());
        repository
            .save_dispatch(&updated)
            .await
            .expect("updated review run should save");

        let loaded = repository
            .get_dispatch(&review.id, "dispatch-1")
            .await
            .expect("review run should load")
            .expect("review run should exist");
        assert_eq!(loaded, updated);
    }

    #[tokio::test]
    async fn dispatches_for_review_and_latest_dispatch_order_by_created_at_desc() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let review = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T09:00:00.000Z",
        );
        review_repository
            .save_review(&review)
            .await
            .expect("review should save");

        let older = sample_review_run(
            "dispatch-older",
            &review,
            RemoteAgentPreferredTool::Codex,
            DispatchStatus::Failed,
            "2026-04-05T09:30:00.000Z",
            "2026-04-05T09:40:00.000Z",
        );
        let newer = sample_review_run(
            "dispatch-newer",
            &review,
            RemoteAgentPreferredTool::Claude,
            DispatchStatus::Running,
            "2026-04-05T10:30:00.000Z",
            "2026-04-05T10:35:00.000Z",
        );
        repository
            .save_dispatch(&older)
            .await
            .expect("older review run should save");
        repository
            .save_dispatch(&newer)
            .await
            .expect("newer review run should save");

        let history = repository
            .dispatches_for_review(&review.id)
            .await
            .expect("review run history should load");
        assert_eq!(
            history
                .iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-newer", "dispatch-older"],
        );

        let latest = repository
            .latest_dispatch_for_review(&review.id)
            .await
            .expect("latest review run should load")
            .expect("latest review run should exist");
        assert_eq!(latest.dispatch_id, "dispatch-newer");
    }

    #[tokio::test]
    async fn list_dispatches_honors_optional_limit() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let review_a = sample_review(
            "review-41",
            41,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T08:00:00.000Z",
            "2026-04-05T08:00:00.000Z",
        );
        let review_b = sample_review(
            "review-42",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T09:00:00.000Z",
        );
        let review_c = sample_review(
            "review-43",
            43,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T10:00:00.000Z",
            "2026-04-05T10:00:00.000Z",
        );
        for review in [&review_a, &review_b, &review_c] {
            review_repository
                .save_review(review)
                .await
                .expect("review should save");
        }

        for record in [
            sample_review_run(
                "dispatch-1",
                &review_a,
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Failed,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ),
            sample_review_run(
                "dispatch-2",
                &review_b,
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ),
            sample_review_run(
                "dispatch-3",
                &review_c,
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Succeeded,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ),
        ] {
            repository
                .save_dispatch(&record)
                .await
                .expect("review run should save");
        }

        let all = repository
            .list_dispatches(None)
            .await
            .expect("review run list should load");
        assert_eq!(
            all.iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-3", "dispatch-2", "dispatch-1"],
        );

        let limited = repository
            .list_dispatches(Some(2))
            .await
            .expect("limited review run list should load");
        assert_eq!(
            limited
                .iter()
                .map(|record| record.dispatch_id.as_str())
                .collect::<Vec<_>>(),
            vec!["dispatch-3", "dispatch-2"],
        );
    }

    #[tokio::test]
    async fn review_ids_with_history_returns_sorted_unique_ids() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let review_a = sample_review(
            "review-a",
            41,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T08:00:00.000Z",
            "2026-04-05T08:00:00.000Z",
        );
        let review_b = sample_review(
            "review-b",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T09:00:00.000Z",
        );
        for review in [&review_b, &review_a] {
            review_repository
                .save_review(review)
                .await
                .expect("review should save");
        }

        for record in [
            sample_review_run(
                "dispatch-1",
                &review_b,
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Preparing,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ),
            sample_review_run(
                "dispatch-2",
                &review_a,
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ),
            sample_review_run(
                "dispatch-3",
                &review_b,
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Succeeded,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ),
        ] {
            repository
                .save_dispatch(&record)
                .await
                .expect("review run should save");
        }

        let review_ids = repository
            .review_ids_with_history()
            .await
            .expect("review ids should load");
        assert_eq!(
            review_ids,
            vec!["review-a".to_owned(), "review-b".to_owned()]
        );
    }

    #[tokio::test]
    async fn delete_dispatch_history_for_review_removes_only_target_rows() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");
        let repository = database.review_dispatch_repository();
        let review_repository = database.review_repository();

        let review_a = sample_review(
            "review-a",
            41,
            RemoteAgentPreferredTool::Codex,
            "2026-04-05T08:00:00.000Z",
            "2026-04-05T08:00:00.000Z",
        );
        let review_b = sample_review(
            "review-b",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T09:00:00.000Z",
            "2026-04-05T09:00:00.000Z",
        );
        for review in [&review_a, &review_b] {
            review_repository
                .save_review(review)
                .await
                .expect("review should save");
        }

        repository
            .save_dispatch(&sample_review_run(
                "dispatch-a1",
                &review_a,
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Failed,
                "2026-04-05T09:00:00.000Z",
                "2026-04-05T09:05:00.000Z",
            ))
            .await
            .expect("review a run should save");
        repository
            .save_dispatch(&sample_review_run(
                "dispatch-b1",
                &review_b,
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ))
            .await
            .expect("review b run should save");

        repository
            .delete_dispatch_history_for_review(&review_a.id)
            .await
            .expect("review a history should delete");

        assert!(repository
            .dispatches_for_review(&review_a.id)
            .await
            .expect("review a history should load")
            .is_empty());
        assert_eq!(
            repository
                .dispatches_for_review(&review_b.id)
                .await
                .expect("review b history should load")
                .len(),
            1,
        );
    }
}
