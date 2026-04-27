use std::collections::BTreeMap;

use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{
    DispatchStatus, RemoteAgentPreferredTool, RemoteAgentReviewOutcome, RemoteRunState,
    ReviewRunRecord,
};

use super::super::lifecycle::refresh::{
    refresh_active_remote_run_records, RemoteRunRefreshAdapter, RemoteRunRefreshMessages,
};
use super::super::lifecycle::snapshot::{RemoteRunSnapshotAction, RemoteRunSnapshotPolicy};
use crate::constants::PREPARING_STALE_AFTER;
use crate::types::ClaudeStructuredOutputEnvelope;
use crate::{RemoteRunSnapshotView, RemoteWorkspace};

use super::RemoteReviewService;

pub(super) async fn refresh_active_review_dispatch_records(
    service: &RemoteReviewService<'_>,
    records: Vec<ReviewRunRecord>,
) -> Result<Vec<ReviewRunRecord>, TrackError> {
    refresh_active_remote_run_records(
        service.config_service,
        service.database,
        &ReviewRunRefreshAdapter { service },
        records,
        PREPARING_STALE_AFTER,
    )
    .await
}

struct ReviewRunRefreshAdapter<'service, 'database> {
    service: &'service RemoteReviewService<'database>,
}

#[async_trait::async_trait]
impl RemoteRunRefreshAdapter for ReviewRunRefreshAdapter<'_, '_> {
    type Record = ReviewRunRecord;

    fn messages(&self) -> RemoteRunRefreshMessages {
        RemoteRunRefreshMessages {
            run_kind: "review_run",
            unavailable_locally_summary:
                "Remote reconciliation is unavailable locally, so active review runs were released.",
            snapshot_load_failed_summary: None,
            missing_snapshot_summary:
                "Remote reconciliation could not find this review run anymore, so it was released locally.",
            missing_snapshot_error: "Remote review snapshot is missing.",
            parse_error_blocked_summary:
                "Remote reconciliation could not confirm this review run, so it was released locally.",
        }
    }

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
        &record.run
    }

    async fn load_snapshots(
        &self,
        workspace: &RemoteWorkspace,
        records: &[Self::Record],
    ) -> Result<BTreeMap<String, RemoteRunSnapshotView>, TrackError> {
        workspace
            .review_runs()
            .load_snapshots_for_active_records(records)
            .await
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .review_dispatch_repository()
            .save_dispatch(record)
            .await
    }

    async fn finalize_locally(
        &self,
        record: &Self::Record,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<Self::Record, TrackError> {
        self.service
            .finalize_review_dispatch_locally(record, status, summary, error_message)
            .await
    }

    fn mark_abandoned_if_preparing_stale(
        &self,
        record: Self::Record,
        refreshed_at: time::OffsetDateTime,
        stale_after: time::Duration,
    ) -> Option<Self::Record> {
        record.mark_abandoned_if_preparing_stale(refreshed_at, stale_after)
    }

    fn mark_failed_from_remote_refresh(
        &self,
        record: Self::Record,
        refreshed_at: time::OffsetDateTime,
        finished_at: time::OffsetDateTime,
        error_message: String,
    ) -> Self::Record {
        record.mark_failed_from_remote_refresh(refreshed_at, finished_at, error_message)
    }

    fn refresh_record_from_snapshot(
        &self,
        record: Self::Record,
        snapshot: &RemoteRunSnapshotView,
    ) -> Result<Self::Record, TrackError> {
        refresh_review_dispatch_record_from_snapshot(record, snapshot)
    }
}

pub(super) fn refresh_review_dispatch_record_from_snapshot(
    record: ReviewRunRecord,
    snapshot: &RemoteRunSnapshotView,
) -> Result<ReviewRunRecord, TrackError> {
    let preferred_tool = record.run.preferred_tool;
    let action =
        review_run_snapshot_policy().reconcile(&record.run, snapshot, |remote_result| {
            parse_remote_review_outcome(preferred_tool, remote_result)
        })?;

    match action {
        RemoteRunSnapshotAction::Unchanged => Ok(record),
        RemoteRunSnapshotAction::MarkRunning { refreshed_at } => {
            Ok(record.mark_running_from_remote(refreshed_at))
        }
        RemoteRunSnapshotAction::MarkCanceled {
            refreshed_at,
            finished_at,
        } => Ok(record.mark_canceled_from_remote(refreshed_at, finished_at)),
        RemoteRunSnapshotAction::MarkFailed {
            refreshed_at,
            finished_at,
            error_message,
        } => Ok(record.mark_failed_from_remote_refresh(refreshed_at, finished_at, error_message)),
        RemoteRunSnapshotAction::ApplyCompletedOutcome {
            refreshed_at,
            finished_at,
            outcome,
        } => Ok(record.apply_remote_review_outcome(outcome, refreshed_at, finished_at)),
    }
}

fn parse_remote_review_outcome(
    preferred_tool: RemoteAgentPreferredTool,
    remote_result: &str,
) -> Result<RemoteAgentReviewOutcome, TrackError> {
    match preferred_tool {
        RemoteAgentPreferredTool::Claude => ClaudeStructuredOutputEnvelope::<
            RemoteAgentReviewOutcome,
        >::parse_result(
            remote_result, preferred_tool, "Remote review result"
        ),
        RemoteAgentPreferredTool::Codex => {
            serde_json::from_str::<RemoteAgentReviewOutcome>(remote_result).map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote review result is not valid JSON: {error}"),
                )
            })
        }
    }
}

fn review_run_snapshot_policy() -> RemoteRunSnapshotPolicy {
    RemoteRunSnapshotPolicy::new(
        PREPARING_STALE_AFTER,
        "Review preparation stopped before the remote agent launched.",
        "Remote review run completed without producing a structured result.",
        "Remote review run failed before returning a structured result.",
    )
}
