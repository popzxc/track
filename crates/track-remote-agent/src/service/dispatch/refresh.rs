use std::collections::BTreeMap;

use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{
    DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentPreferredTool, RemoteRunState,
    TaskDispatchRecord,
};

use super::super::lifecycle::refresh::{
    refresh_active_remote_run_records, RemoteRunRefreshAdapter, RemoteRunRefreshMessages,
};
use super::super::lifecycle::snapshot::{RemoteRunSnapshotAction, RemoteRunSnapshotPolicy};
use crate::constants::PREPARING_STALE_AFTER;
use crate::types::ClaudeStructuredOutputEnvelope;
use crate::{RemoteRunSnapshotView, RemoteWorkspace};

use super::RemoteDispatchService;

pub(super) async fn refresh_active_dispatch_records(
    service: &RemoteDispatchService<'_>,
    records: Vec<TaskDispatchRecord>,
) -> Result<Vec<TaskDispatchRecord>, TrackError> {
    refresh_active_remote_run_records(
        service.config_service,
        service.database,
        &TaskDispatchRefreshAdapter { service },
        records,
    )
    .await
}

struct TaskDispatchRefreshAdapter<'service, 'database> {
    service: &'service RemoteDispatchService<'database>,
}

#[async_trait::async_trait]
impl RemoteRunRefreshAdapter for TaskDispatchRefreshAdapter<'_, '_> {
    type Record = TaskDispatchRecord;

    fn messages(&self) -> RemoteRunRefreshMessages {
        RemoteRunRefreshMessages {
            run_kind: "task_dispatch",
            unavailable_locally_summary:
                "Remote reconciliation is unavailable locally, so active runs were released.",
            snapshot_load_failed_summary: Some(
                "Remote reconciliation could not reach the remote machine, so active runs were released locally.",
            ),
            parse_error_blocked_summary:
                "Remote reconciliation could not confirm this run, so it was released locally.",
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
            .task_runs()
            .load_snapshots_for_active_records(records)
            .await
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .dispatch_repository()
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
            .finalize_dispatch_locally(record, status, summary, error_message)
            .await
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
        refresh_dispatch_record_from_snapshot(record, snapshot)
    }
}

pub(in crate::service) fn refresh_dispatch_record_from_snapshot(
    record: TaskDispatchRecord,
    snapshot: &RemoteRunSnapshotView,
) -> Result<TaskDispatchRecord, TrackError> {
    let preferred_tool = record.run.preferred_tool;
    let action = task_run_snapshot_policy().reconcile(&record.run, snapshot, |remote_result| {
        parse_remote_dispatch_outcome(preferred_tool, remote_result)
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
        } => Ok(record.apply_remote_dispatch_outcome(outcome, refreshed_at, finished_at)),
    }
}

fn parse_remote_dispatch_outcome(
    preferred_tool: RemoteAgentPreferredTool,
    remote_result: &str,
) -> Result<RemoteAgentDispatchOutcome, TrackError> {
    match preferred_tool {
        RemoteAgentPreferredTool::Claude => {
            type DispatchEnvelope = ClaudeStructuredOutputEnvelope<RemoteAgentDispatchOutcome>;
            DispatchEnvelope::parse_result(remote_result, preferred_tool, "Remote agent result")
        }
        RemoteAgentPreferredTool::Codex => {
            serde_json::from_str::<RemoteAgentDispatchOutcome>(remote_result).map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote agent result is not valid JSON: {error}"),
                )
            })
        }
    }
}

fn task_run_snapshot_policy() -> RemoteRunSnapshotPolicy {
    RemoteRunSnapshotPolicy::new(
        PREPARING_STALE_AFTER,
        "Dispatch preparation stopped before the remote agent launched.",
        "Remote agent run completed without producing a structured result.",
        "Remote agent run failed before returning a structured result.",
    )
}
