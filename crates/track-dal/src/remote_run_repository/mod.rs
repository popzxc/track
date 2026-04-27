mod records;

use track_types::errors::TrackError;
use track_types::ids::{DispatchId, ReviewId, TaskId};
use track_types::types::{ActiveRemoteRun, DispatchStatus, RemoteRunKind, RemoteRunOwner};

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct RemoteRunRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> RemoteRunRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    pub async fn active_remote_runs(&self) -> Result<Vec<ActiveRemoteRun>, TrackError> {
        let mut connection = self.database.connect().await?;
        let rows = sqlx::query_as::<_, records::ActiveRemoteRunRow>(
            r#"
            SELECT
                rr.dispatch_id,
                rr.kind,
                CASE rr.kind
                    WHEN 'task' THEN td.task_id
                    WHEN 'review' THEN rd.review_id
                END AS owner_id,
                rr.status
            FROM remote_runs rr
            LEFT JOIN task_run_details td
                ON rr.kind = 'task' AND rr.dispatch_id = td.dispatch_id
            LEFT JOIN review_run_details rd
                ON rr.kind = 'review' AND rr.dispatch_id = rd.dispatch_id
            WHERE rr.status IN (?1, ?2)
            ORDER BY rr.created_at DESC, rr.dispatch_id DESC
            "#,
        )
        .bind(DispatchStatus::Preparing.as_str())
        .bind(DispatchStatus::Running.as_str())
        .fetch_all(&mut *connection)
        .await
        .database_error_with("Could not load active remote runs")?;

        Ok(rows.into_iter().map(ActiveRemoteRun::from).collect())
    }
}

impl From<records::ActiveRemoteRunRow> for ActiveRemoteRun {
    fn from(row: records::ActiveRemoteRunRow) -> Self {
        let kind =
            RemoteRunKind::from_str(row.kind.as_str()).expect("stored run kinds should be valid");
        let status = DispatchStatus::from_str(row.status.as_str())
            .expect("stored run statuses should be valid");
        let owner_id = row
            .owner_id
            .expect("stored active remote runs should have an owner row");
        let owner = match kind {
            RemoteRunKind::Task => RemoteRunOwner::Task(TaskId::from_db(owner_id)),
            RemoteRunKind::Review => RemoteRunOwner::Review(ReviewId::from_db(owner_id)),
        };

        Self {
            dispatch_id: DispatchId::from_db(row.dispatch_id),
            kind,
            owner,
            status,
        }
    }
}

#[cfg(test)]
mod tests {
    use track_types::types::{RemoteAgentPreferredTool, RemoteRunOwner};

    use crate::test_support::{
        sample_dispatch, sample_review, sample_review_run, temporary_database_path,
    };

    use super::*;

    #[tokio::test]
    async fn active_remote_runs_returns_task_and_review_blockers_only() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should open");

        database
            .dispatch_repository()
            .save_dispatch(&sample_dispatch(
                "dispatch-running-task",
                "task-a",
                "project-a",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Running,
                "2026-04-05T10:00:00.000Z",
                "2026-04-05T10:05:00.000Z",
            ))
            .await
            .expect("running task dispatch should save");
        database
            .dispatch_repository()
            .save_dispatch(&sample_dispatch(
                "dispatch-finished-task",
                "task-b",
                "project-b",
                RemoteAgentPreferredTool::Codex,
                DispatchStatus::Succeeded,
                "2026-04-05T11:00:00.000Z",
                "2026-04-05T11:05:00.000Z",
            ))
            .await
            .expect("finished task dispatch should save");

        let review = sample_review(
            "review-a",
            42,
            RemoteAgentPreferredTool::Claude,
            "2026-04-05T12:00:00.000Z",
            "2026-04-05T12:00:00.000Z",
        );
        database
            .review_repository()
            .save_review(&review)
            .await
            .expect("review should save");
        database
            .review_dispatch_repository()
            .save_dispatch(&sample_review_run(
                "dispatch-preparing-review",
                &review,
                RemoteAgentPreferredTool::Claude,
                DispatchStatus::Preparing,
                "2026-04-05T12:30:00.000Z",
                "2026-04-05T12:35:00.000Z",
            ))
            .await
            .expect("preparing review run should save");

        let blockers = database
            .remote_run_repository()
            .active_remote_runs()
            .await
            .expect("active remote runs should load");

        assert_eq!(blockers.len(), 2);
        assert_eq!(
            blockers[0].dispatch_id.as_str(),
            "dispatch-preparing-review"
        );
        assert_eq!(blockers[0].kind, RemoteRunKind::Review);
        assert!(
            matches!(&blockers[0].owner, RemoteRunOwner::Review(id) if id.as_str() == "review-a")
        );
        assert_eq!(blockers[1].dispatch_id.as_str(), "dispatch-running-task");
        assert_eq!(blockers[1].kind, RemoteRunKind::Task);
        assert!(matches!(&blockers[1].owner, RemoteRunOwner::Task(id) if id.as_str() == "task-a"));
    }
}
