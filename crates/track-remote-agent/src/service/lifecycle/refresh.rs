use std::collections::BTreeMap;

use time::{Duration, OffsetDateTime};
use track_dal::database::DatabaseContext;
use track_types::errors::TrackError;
use track_types::types::{DispatchStatus, RemoteRunState};

use crate::{RemoteRunSnapshotView, RemoteWorkspace};

use super::super::log_remote_failure_output;
use super::super::remote_agent_services::{
    load_refresh_remote_workspace, RefreshRemoteWorkspace, RemoteAgentConfigProvider,
};

// =============================================================================
// Remote Run Refresh Orchestration
// =============================================================================
//
// Task dispatches and PR review runs have different persistence and completion
// payloads, but their refresh loop is the same: load remote snapshots, reconcile
// each active run, persist changes, and gracefully release local state when the
// remote workspace can no longer be trusted. The adapter keeps the domain edges
// explicit so the shared loop does not need to know what a task or review is.
#[async_trait::async_trait]
pub(in crate::service) trait RemoteRunRefreshAdapter: Sync {
    type Record: Clone + PartialEq + Send + Sync;

    fn messages(&self) -> RemoteRunRefreshMessages;

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState;

    async fn load_snapshots(
        &self,
        workspace: &RemoteWorkspace,
        records: &[Self::Record],
    ) -> Result<BTreeMap<String, RemoteRunSnapshotView>, TrackError>;

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError>;

    async fn finalize_locally(
        &self,
        record: &Self::Record,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<Self::Record, TrackError>;

    fn mark_abandoned_if_preparing_stale(
        &self,
        record: Self::Record,
        refreshed_at: OffsetDateTime,
        stale_after: Duration,
    ) -> Option<Self::Record>;

    fn mark_failed_from_remote_refresh(
        &self,
        record: Self::Record,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
        error_message: String,
    ) -> Self::Record;

    fn refresh_record_from_snapshot(
        &self,
        record: Self::Record,
        snapshot: &RemoteRunSnapshotView,
    ) -> Result<Self::Record, TrackError>;
}

#[derive(Debug, Clone, Copy)]
pub(in crate::service) struct RemoteRunRefreshMessages {
    pub(in crate::service) run_kind: &'static str,
    pub(in crate::service) unavailable_locally_summary: &'static str,
    pub(in crate::service) snapshot_load_failed_summary: Option<&'static str>,
    pub(in crate::service) missing_snapshot_summary: &'static str,
    pub(in crate::service) missing_snapshot_error: &'static str,
    pub(in crate::service) parse_error_blocked_summary: &'static str,
}

pub(in crate::service) async fn refresh_active_remote_run_records<Adapter>(
    config_service: &dyn RemoteAgentConfigProvider,
    database: &DatabaseContext,
    adapter: &Adapter,
    records: Vec<Adapter::Record>,
    stale_after: Duration,
) -> Result<Vec<Adapter::Record>, TrackError>
where
    Adapter: RemoteRunRefreshAdapter,
{
    let messages = adapter.messages();
    let workspace = match load_refresh_remote_workspace(config_service, database).await? {
        RefreshRemoteWorkspace::Available(workspace) => workspace,
        RefreshRemoteWorkspace::UnavailableLocally { error_message } => {
            return release_active_runs_after_reconciliation_loss(
                adapter,
                records,
                messages.unavailable_locally_summary,
                &error_message,
            )
            .await;
        }
    };
    let snapshots_by_dispatch_id = match adapter.load_snapshots(&workspace, &records).await {
        Ok(snapshots) => snapshots,
        Err(error) => {
            let Some(summary) = messages.snapshot_load_failed_summary else {
                return Err(error);
            };
            let error_message = error.to_string();
            return release_active_runs_after_reconciliation_loss(
                adapter,
                records,
                summary,
                &error_message,
            )
            .await;
        }
    };

    let mut refreshed_records = Vec::with_capacity(records.len());
    for record in records {
        if !adapter.run(&record).status.is_active() {
            refreshed_records.push(record);
            continue;
        }

        let dispatch_id = adapter.run(&record).dispatch_id.as_str();
        let Some(snapshot) = snapshots_by_dispatch_id.get(dispatch_id) else {
            refreshed_records.push(
                refresh_record_after_missing_snapshot(adapter, record, messages, stale_after)
                    .await?,
            );
            continue;
        };

        refreshed_records
            .push(refresh_record_from_present_snapshot(adapter, record, snapshot, messages).await?);
    }

    Ok(refreshed_records)
}

async fn release_active_runs_after_reconciliation_loss<Adapter>(
    adapter: &Adapter,
    records: Vec<Adapter::Record>,
    summary: &str,
    error_message: &str,
) -> Result<Vec<Adapter::Record>, TrackError>
where
    Adapter: RemoteRunRefreshAdapter,
{
    let mut refreshed_records = Vec::with_capacity(records.len());
    for record in records {
        if adapter.run(&record).status.is_active() {
            refreshed_records.push(
                adapter
                    .finalize_locally(
                        &record,
                        DispatchStatus::Blocked,
                        summary,
                        Some(error_message),
                    )
                    .await?,
            );
        } else {
            refreshed_records.push(record);
        }
    }

    Ok(refreshed_records)
}

async fn refresh_record_after_missing_snapshot<Adapter>(
    adapter: &Adapter,
    record: Adapter::Record,
    messages: RemoteRunRefreshMessages,
    stale_after: Duration,
) -> Result<Adapter::Record, TrackError>
where
    Adapter: RemoteRunRefreshAdapter,
{
    let dispatch_id = adapter.run(&record).dispatch_id.to_string();
    if let Some(updated) = adapter.mark_abandoned_if_preparing_stale(
        record.clone(),
        track_types::time_utils::now_utc(),
        stale_after,
    ) {
        adapter.save_record(&updated).await?;
        tracing::info!(
            run_kind = messages.run_kind,
            dispatch_id = %adapter.run(&updated).dispatch_id,
            status = ?adapter.run(&updated).status,
            "Marked stale preparing remote run as abandoned during refresh"
        );
        return Ok(updated);
    }

    let updated = adapter
        .finalize_locally(
            &record,
            DispatchStatus::Blocked,
            messages.missing_snapshot_summary,
            Some(messages.missing_snapshot_error),
        )
        .await?;
    tracing::warn!(
        run_kind = messages.run_kind,
        dispatch_id = %dispatch_id,
        status = ?adapter.run(&updated).status,
        "Released active remote run because its snapshot is missing"
    );

    Ok(updated)
}

async fn refresh_record_from_present_snapshot<Adapter>(
    adapter: &Adapter,
    record: Adapter::Record,
    snapshot: &RemoteRunSnapshotView,
    messages: RemoteRunRefreshMessages,
) -> Result<Adapter::Record, TrackError>
where
    Adapter: RemoteRunRefreshAdapter,
{
    match adapter.refresh_record_from_snapshot(record.clone(), snapshot) {
        Ok(updated) => {
            if updated != record {
                adapter.save_record(&updated).await?;
                tracing::info!(
                    run_kind = messages.run_kind,
                    dispatch_id = %adapter.run(&updated).dispatch_id,
                    previous_status = ?adapter.run(&record).status,
                    status = ?adapter.run(&updated).status,
                    "Refreshed remote run state from remote snapshot"
                );
            }
            Ok(updated)
        }
        Err(error) => {
            tracing::error!(
                run_kind = messages.run_kind,
                dispatch_id = %adapter.run(&record).dispatch_id,
                error = %error,
                observed_status = ?snapshot.status,
                "Failed to interpret remote run snapshot"
            );
            log_remote_failure_output(snapshot.result.as_deref(), snapshot.stderr.as_deref());
            if snapshot.is_finished() {
                let refreshed_at = track_types::time_utils::now_utc();
                let finished_at = snapshot.finished_at_or(refreshed_at);
                let updated = adapter.mark_failed_from_remote_refresh(
                    record,
                    refreshed_at,
                    finished_at,
                    error.to_string(),
                );
                adapter.save_record(&updated).await?;
                tracing::warn!(
                    run_kind = messages.run_kind,
                    dispatch_id = %adapter.run(&updated).dispatch_id,
                    status = ?adapter.run(&updated).status,
                    "Marked remote run as failed after snapshot parse error"
                );
                Ok(updated)
            } else {
                let error_message = error.to_string();
                let updated = adapter
                    .finalize_locally(
                        &record,
                        DispatchStatus::Blocked,
                        messages.parse_error_blocked_summary,
                        Some(&error_message),
                    )
                    .await?;
                tracing::warn!(
                    run_kind = messages.run_kind,
                    dispatch_id = %adapter.run(&updated).dispatch_id,
                    status = ?adapter.run(&updated).status,
                    "Released active remote run after reconciliation error"
                );
                Ok(updated)
            }
        }
    }
}
