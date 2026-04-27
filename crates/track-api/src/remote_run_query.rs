use track_types::errors::TrackError;
use track_types::ids::{ReviewId, TaskId};
use track_types::types::{ReviewRunRecord, TaskDispatchRecord};

use crate::AppState;

// Remote run reads have a split personality: terminal records are ordinary
// trusted local state, while active records need reconciliation against the
// untrusted remote workspace before the UI acts on them. Keeping that rule in
// one API-side service lets route handlers stay local to HTTP concerns without
// weakening the remote-runtime invariant in track-remote-agent.
pub(crate) struct RemoteRunQueryService<'state> {
    state: &'state AppState,
}

impl<'state> RemoteRunQueryService<'state> {
    pub(crate) fn new(state: &'state AppState) -> Self {
        Self { state }
    }

    pub(crate) async fn latest_task_dispatches(
        &self,
        task_ids: &[TaskId],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self
            .state
            .database
            .dispatch_repository()
            .latest_dispatches_for_tasks(task_ids)
            .await?;
        self.refresh_task_dispatches_if_active(records).await
    }

    pub(crate) async fn task_dispatch_history(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self
            .state
            .database
            .dispatch_repository()
            .dispatches_for_task(task_id)
            .await?;
        self.refresh_task_dispatches_if_active(records).await
    }

    pub(crate) async fn global_task_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self
            .state
            .database
            .dispatch_repository()
            .list_dispatches(limit)
            .await?;
        self.refresh_task_dispatches_if_active(records).await
    }

    pub(crate) async fn latest_review_runs(
        &self,
        review_ids: &[ReviewId],
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self
            .state
            .database
            .review_dispatch_repository()
            .latest_dispatches_for_reviews(review_ids)
            .await?;
        self.refresh_review_runs_if_active(records).await
    }

    pub(crate) async fn review_run_history(
        &self,
        review_id: &ReviewId,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self
            .state
            .database
            .review_dispatch_repository()
            .dispatches_for_review(review_id)
            .await?;
        self.refresh_review_runs_if_active(records).await
    }

    async fn refresh_task_dispatches_if_active(
        &self,
        records: Vec<TaskDispatchRecord>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        if records.iter().all(|record| !record.run.status.is_active()) {
            return Ok(records);
        }

        let _remote_agent_operation_guard = self.state.remote_agent_operation_guard().await;
        self.state
            .remote_agent_runtime_services()
            .await?
            .dispatch()
            .refresh_active_dispatch_records(records)
            .await
    }

    async fn refresh_review_runs_if_active(
        &self,
        records: Vec<ReviewRunRecord>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        if records.iter().all(|record| !record.run.status.is_active()) {
            return Ok(records);
        }

        let _remote_agent_operation_guard = self.state.remote_agent_operation_guard().await;
        self.state
            .remote_agent_runtime_services()
            .await?
            .review()
            .refresh_active_review_dispatch_records(records)
            .await
    }
}
