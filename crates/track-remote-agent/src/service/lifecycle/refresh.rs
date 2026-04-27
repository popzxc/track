use std::collections::BTreeMap;

use time::OffsetDateTime;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{DispatchStatus, RemoteRunState};

use crate::{RemoteRunSnapshotView, RemoteWorkspace};

use super::super::log_remote_failure_output;

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
    pub(in crate::service) snapshot_load_failed_summary: Option<&'static str>,
    pub(in crate::service) parse_error_blocked_summary: &'static str,
}

pub(in crate::service) async fn refresh_active_remote_run_records<Adapter>(
    workspace: &RemoteWorkspace,
    adapter: &Adapter,
    records: Vec<Adapter::Record>,
) -> Result<Vec<Adapter::Record>, TrackError>
where
    Adapter: RemoteRunRefreshAdapter,
{
    let messages = adapter.messages();
    if records
        .iter()
        .all(|record| !adapter.run(record).status.is_active())
    {
        return Ok(records);
    }

    let snapshots_by_dispatch_id = match adapter.load_snapshots(workspace, &records).await {
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
        let snapshot = snapshots_by_dispatch_id
            .get(dispatch_id)
            .ok_or_else(|| missing_snapshot_contract_error(messages.run_kind, dispatch_id))?;

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

fn missing_snapshot_contract_error(run_kind: &str, dispatch_id: &str) -> TrackError {
    TrackError::new(
        ErrorCode::InternalError,
        format!(
            "Remote {run_kind} refresh adapter did not return a snapshot entry for active run {dispatch_id}. Adapters must return an explicit missing snapshot instead of omitting active runs.",
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;
    use track_config::runtime::RemoteAgentRuntimeConfig;
    use track_dal::database::DatabaseContext;
    use track_types::ids::DispatchId;
    use track_types::time_utils::now_utc;
    use track_types::types::RemoteAgentPreferredTool;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestRecord {
        run: RemoteRunState,
    }

    #[derive(Debug, Default)]
    struct TestRefreshState {
        saved_records: usize,
        finalized_records: usize,
        interpreted_snapshots: usize,
    }

    #[derive(Debug, Clone)]
    struct TestRefreshAdapter {
        state: Arc<Mutex<TestRefreshState>>,
    }

    impl TestRefreshAdapter {
        fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(TestRefreshState::default())),
            }
        }

        fn state(&self) -> std::sync::MutexGuard<'_, TestRefreshState> {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }

    #[async_trait::async_trait]
    impl RemoteRunRefreshAdapter for TestRefreshAdapter {
        type Record = TestRecord;

        fn messages(&self) -> RemoteRunRefreshMessages {
            RemoteRunRefreshMessages {
                run_kind: "test_run",
                snapshot_load_failed_summary: None,
                parse_error_blocked_summary: "The test run snapshot could not be parsed.",
            }
        }

        fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
            &record.run
        }

        async fn load_snapshots(
            &self,
            _workspace: &RemoteWorkspace,
            _records: &[Self::Record],
        ) -> Result<BTreeMap<String, RemoteRunSnapshotView>, TrackError> {
            Ok(BTreeMap::new())
        }

        async fn save_record(&self, _record: &Self::Record) -> Result<(), TrackError> {
            self.state().saved_records += 1;
            Ok(())
        }

        async fn finalize_locally(
            &self,
            record: &Self::Record,
            status: DispatchStatus,
            _summary: &str,
            _error_message: Option<&str>,
        ) -> Result<Self::Record, TrackError> {
            self.state().finalized_records += 1;
            let mut updated = record.clone();
            updated.run.status = status;
            Ok(updated)
        }

        fn mark_failed_from_remote_refresh(
            &self,
            mut record: Self::Record,
            refreshed_at: OffsetDateTime,
            finished_at: OffsetDateTime,
            error_message: String,
        ) -> Self::Record {
            record.run = record.run.mark_failed_from_remote_refresh(
                refreshed_at,
                finished_at,
                error_message,
            );
            record
        }

        fn refresh_record_from_snapshot(
            &self,
            record: Self::Record,
            _snapshot: &RemoteRunSnapshotView,
        ) -> Result<Self::Record, TrackError> {
            self.state().interpreted_snapshots += 1;
            Ok(record)
        }
    }

    fn test_record() -> TestRecord {
        let timestamp = now_utc();
        TestRecord {
            run: RemoteRunState {
                dispatch_id: DispatchId::new("dispatch-1").expect("dispatch id should parse"),
                preferred_tool: RemoteAgentPreferredTool::Codex,
                status: DispatchStatus::Preparing,
                created_at: timestamp,
                updated_at: timestamp,
                finished_at: None,
                remote_host: "198.51.100.10".to_owned(),
                branch_name: None,
                worktree_path: None,
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: None,
            },
        }
    }

    fn test_remote_agent(directory: &TempDir) -> RemoteAgentRuntimeConfig {
        let managed_key_path = directory.path().join("id_ed25519");
        fs::write(&managed_key_path, "test-key").expect("managed key should be created");

        RemoteAgentRuntimeConfig {
            host: "198.51.100.10".to_owned(),
            user: "track".to_owned(),
            port: 22,
            workspace_root: "~/workspace".to_owned(),
            projects_registry_path: "~/workspace/projects.json".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some("export PATH=/usr/local/bin:$PATH".to_owned()),
            review_follow_up: None,
            managed_key_path,
            managed_known_hosts_path: directory.path().join("known_hosts"),
        }
    }

    async fn test_database(database_path: PathBuf) -> DatabaseContext {
        DatabaseContext::initialized(Some(database_path))
            .await
            .expect("test database should initialize")
    }

    #[tokio::test]
    async fn omitted_active_snapshot_is_a_refresh_adapter_contract_error() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = test_database(directory.path().join("track.sqlite")).await;
        let workspace = RemoteWorkspace::new(test_remote_agent(&directory), database.clone())
            .expect("workspace should initialize");
        let adapter = TestRefreshAdapter::new();

        let error = refresh_active_remote_run_records(&workspace, &adapter, vec![test_record()])
            .await
            .expect_err("omitted active snapshot should fail the adapter contract");

        assert_eq!(error.code, ErrorCode::InternalError);
        assert!(error
            .message()
            .contains("did not return a snapshot entry for active run dispatch-1"));
        assert!(error
            .message()
            .contains("return an explicit missing snapshot"));

        let state = adapter.state();
        assert_eq!(state.saved_records, 0);
        assert_eq!(state.finalized_records, 0);
        assert_eq!(state.interpreted_snapshots, 0);
    }
}
