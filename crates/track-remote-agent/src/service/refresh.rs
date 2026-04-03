use std::collections::BTreeMap;

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::now_utc;
use track_types::types::{
    DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentReviewOutcome, ReviewRunRecord,
    TaskDispatchRecord,
};

use crate::constants::{
    PREPARING_STALE_AFTER, REVIEW_RUN_DIRECTORY_NAME, REVIEW_WORKTREE_DIRECTORY_NAME,
};
use crate::remote_actions::ReadDispatchSnapshotsAction;
use crate::ssh::SshClient;
use crate::types::{ClaudeStructuredOutputEnvelope, RemoteDispatchSnapshot};

use super::{RemoteAgentConfigProvider, RemoteDispatchService, RemoteReviewService};

enum RefreshRemoteClient {
    Available(SshClient),
    UnavailableLocally { error_message: String },
}

fn load_refresh_ssh_client(
    config_service: &dyn RemoteAgentConfigProvider,
) -> Result<RefreshRemoteClient, TrackError> {
    let remote_agent = match config_service.load_remote_agent_runtime_config() {
        Ok(Some(config)) => config,
        Ok(None) => {
            return Ok(RefreshRemoteClient::UnavailableLocally {
                error_message: "Remote agent configuration is missing locally.".to_owned(),
            });
        }
        Err(error) if error.remote_unavailable() => {
            return Ok(RefreshRemoteClient::UnavailableLocally {
                error_message: error.to_string(),
            });
        }
        Err(error) => return Err(error),
    };

    if !remote_agent.managed_key_path.exists() {
        return Ok(RefreshRemoteClient::UnavailableLocally {
            error_message: format!(
                "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                collapse_home_path(&remote_agent.managed_key_path)
            ),
        });
    }

    Ok(RefreshRemoteClient::Available(SshClient::new(
        &remote_agent,
    )?))
}

impl<'a> RemoteDispatchService<'a> {
    pub(super) fn refresh_active_dispatch_records(
        &self,
        records: Vec<TaskDispatchRecord>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let ssh_client = match load_refresh_ssh_client(self.config_service)? {
            RefreshRemoteClient::Available(ssh_client) => ssh_client,
            RefreshRemoteClient::UnavailableLocally { error_message } => {
                return self.release_active_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation is unavailable locally, so active runs were released.",
                    &error_message,
                );
            }
        };
        let snapshots_by_dispatch_id = match load_dispatch_snapshots_for_records(
            &ssh_client,
            &records,
        ) {
            Ok(snapshots) => snapshots,
            Err(error) => {
                let error_message = error.to_string();
                return self.release_active_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation could not reach the remote machine, so active runs were released locally.",
                    &error_message,
                );
            }
        };
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(&record.dispatch_id) else {
                if let Some(updated) = record
                    .clone()
                    .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
                {
                    self.dispatch_repository.save_dispatch(&updated)?;
                    refreshed_records.push(updated);
                } else {
                    let updated = self.finalize_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        "Remote reconciliation could not find this run anymore, so it was released locally.",
                        Some("Remote dispatch snapshot is missing."),
                    )?;
                    refreshed_records.push(updated);
                }
                continue;
            };

            match refresh_dispatch_record_from_snapshot(record.clone(), snapshot) {
                Ok(updated) => {
                    if updated != record {
                        self.dispatch_repository.save_dispatch(&updated)?;
                    }
                    refreshed_records.push(updated);
                }
                Err(error) => {
                    if snapshot.is_finished() {
                        let refreshed_at = now_utc();
                        let finished_at = snapshot.finished_at_or(refreshed_at);
                        let updated = record.clone().mark_failed_from_remote_refresh(
                            refreshed_at,
                            finished_at,
                            error.to_string(),
                        );
                        self.dispatch_repository.save_dispatch(&updated)?;
                        refreshed_records.push(updated);
                    } else {
                        let error_message = error.to_string();
                        let updated = self.finalize_dispatch_locally(
                            &record,
                            DispatchStatus::Blocked,
                            "Remote reconciliation could not confirm this run, so it was released locally.",
                            Some(&error_message),
                        )?;
                        refreshed_records.push(updated);
                    }
                }
            }
        }

        Ok(refreshed_records)
    }

    fn release_active_dispatches_after_reconciliation_loss(
        &self,
        records: Vec<TaskDispatchRecord>,
        summary: &str,
        error_message: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if record.status.is_active() {
                refreshed_records.push(self.finalize_dispatch_locally(
                    &record,
                    DispatchStatus::Blocked,
                    summary,
                    Some(error_message),
                )?);
            } else {
                refreshed_records.push(record);
            }
        }

        Ok(refreshed_records)
    }
}

impl<'a> RemoteReviewService<'a> {
    pub(super) fn refresh_active_review_dispatch_records(
        &self,
        records: Vec<ReviewRunRecord>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let ssh_client = match load_refresh_ssh_client(self.config_service)? {
            RefreshRemoteClient::Available(ssh_client) => ssh_client,
            RefreshRemoteClient::UnavailableLocally { error_message } => {
                return self.release_active_review_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation is unavailable locally, so active review runs were released.",
                    &error_message,
                );
            }
        };
        let snapshots_by_dispatch_id = load_review_snapshots_for_records(&ssh_client, &records)?;
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(&record.dispatch_id) else {
                if let Some(updated) = record
                    .clone()
                    .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
                {
                    self.review_dispatch_repository.save_dispatch(&updated)?;
                    refreshed_records.push(updated);
                } else {
                    let updated = self.finalize_review_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        "Remote reconciliation could not find this review run anymore, so it was released locally.",
                        Some("Remote review snapshot is missing."),
                    )?;
                    refreshed_records.push(updated);
                }
                continue;
            };

            match self.refresh_review_dispatch_record_from_snapshot(record.clone(), snapshot) {
                Ok(updated) => {
                    if updated != record {
                        self.review_dispatch_repository.save_dispatch(&updated)?;
                    }
                    refreshed_records.push(updated);
                }
                Err(error) => {
                    if snapshot.is_finished() {
                        let refreshed_at = now_utc();
                        let finished_at = snapshot.finished_at_or(refreshed_at);
                        let updated = record.clone().mark_failed_from_remote_refresh(
                            refreshed_at,
                            finished_at,
                            error.to_string(),
                        );
                        self.review_dispatch_repository.save_dispatch(&updated)?;
                        refreshed_records.push(updated);
                    } else {
                        let error_message = error.to_string();
                        let updated = self.finalize_review_dispatch_locally(
                            &record,
                            DispatchStatus::Blocked,
                            "Remote reconciliation could not confirm this review run, so it was released locally.",
                            Some(&error_message),
                        )?;
                        refreshed_records.push(updated);
                    }
                }
            }
        }

        Ok(refreshed_records)
    }

    pub(super) fn refresh_review_dispatch_record_from_snapshot(
        &self,
        record: ReviewRunRecord,
        snapshot: &RemoteDispatchSnapshot,
    ) -> Result<ReviewRunRecord, TrackError> {
        if snapshot.is_missing() {
            if let Some(updated) = record
                .clone()
                .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
            {
                return Ok(updated);
            }

            return Ok(record);
        }

        if snapshot.is_running() {
            return Ok(record.mark_running_from_remote(now_utc()));
        }

        if snapshot.is_canceled() {
            let refreshed_at = now_utc();
            let finished_at = snapshot.finished_at_or(refreshed_at);
            return Ok(record.mark_canceled_from_remote(refreshed_at, finished_at));
        }

        let refreshed_at = now_utc();
        let finished_at = snapshot.finished_at_or(refreshed_at);
        if snapshot.is_completed() {
            let remote_result = snapshot.required_result(
                "Remote review run completed without producing a structured result.",
            )?;
            let outcome = ClaudeStructuredOutputEnvelope::<RemoteAgentReviewOutcome>::parse_result(
                remote_result,
                record.preferred_tool,
                "Remote review result",
            )?;
            return Ok(record.apply_remote_review_outcome(outcome, refreshed_at, finished_at));
        }

        Ok(record.mark_failed_from_remote_refresh(
            refreshed_at,
            finished_at,
            snapshot
                .failure_message("Remote review run failed before returning a structured result."),
        ))
    }

    fn release_active_review_dispatches_after_reconciliation_loss(
        &self,
        records: Vec<ReviewRunRecord>,
        summary: &str,
        error_message: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if record.status.is_active() {
                refreshed_records.push(self.finalize_review_dispatch_locally(
                    &record,
                    DispatchStatus::Blocked,
                    summary,
                    Some(error_message),
                )?);
            } else {
                refreshed_records.push(record);
            }
        }

        Ok(refreshed_records)
    }
}

pub(crate) fn load_dispatch_snapshots_for_records(
    ssh_client: &SshClient,
    records: &[TaskDispatchRecord],
) -> Result<BTreeMap<String, RemoteDispatchSnapshot>, TrackError> {
    let mut dispatch_ids = Vec::new();
    let mut run_directories = Vec::new();

    for record in records {
        if !record.status.is_active() {
            continue;
        }

        let Some(worktree_path) = record.worktree_path.as_deref() else {
            continue;
        };
        let Ok(run_directory) = derive_remote_run_directory(worktree_path, &record.dispatch_id)
        else {
            continue;
        };

        dispatch_ids.push(record.dispatch_id.clone());
        run_directories.push(run_directory);
    }

    if run_directories.is_empty() {
        return Ok(BTreeMap::new());
    }

    let snapshots = ReadDispatchSnapshotsAction::new(ssh_client, &run_directories).execute()?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}

pub(crate) fn derive_remote_run_directory(
    worktree_path: &str,
    dispatch_id: &str,
) -> Result<String, TrackError> {
    worktree_path
        .rsplit_once("/worktrees/")
        .map(|(prefix, _suffix)| format!("{prefix}/dispatches/{dispatch_id}"))
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not derive the remote run directory from the worktree path.",
            )
        })
}

pub(crate) fn derive_remote_run_directory_for_record(
    record: &TaskDispatchRecord,
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Option<String> {
    if let Some(worktree_path) = record.worktree_path.as_deref() {
        if let Ok(run_directory) = derive_remote_run_directory(worktree_path, &record.dispatch_id) {
            return Some(run_directory);
        }
    }

    if record.project.trim().is_empty() || remote_agent.workspace_root.trim().is_empty() {
        return None;
    }

    Some(format!(
        "{}/{}/dispatches/{}",
        remote_agent.workspace_root.trim_end_matches('/'),
        record.project,
        record.dispatch_id
    ))
}

pub(crate) fn load_review_snapshots_for_records(
    ssh_client: &SshClient,
    records: &[ReviewRunRecord],
) -> Result<BTreeMap<String, RemoteDispatchSnapshot>, TrackError> {
    let mut dispatch_ids = Vec::new();
    let mut run_directories = Vec::new();

    for record in records {
        if !record.status.is_active() {
            continue;
        }

        let Some(worktree_path) = record.worktree_path.as_deref() else {
            continue;
        };
        let Ok(run_directory) = derive_review_run_directory(worktree_path, &record.dispatch_id)
        else {
            continue;
        };

        dispatch_ids.push(record.dispatch_id.clone());
        run_directories.push(run_directory);
    }

    if run_directories.is_empty() {
        return Ok(BTreeMap::new());
    }

    let snapshots = ReadDispatchSnapshotsAction::new(ssh_client, &run_directories).execute()?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}

pub(crate) fn derive_review_run_directory(
    worktree_path: &str,
    dispatch_id: &str,
) -> Result<String, TrackError> {
    worktree_path
        .rsplit_once(&format!("/{REVIEW_WORKTREE_DIRECTORY_NAME}/"))
        .map(|(prefix, _suffix)| format!("{prefix}/{REVIEW_RUN_DIRECTORY_NAME}/{dispatch_id}"))
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not derive the remote review run directory from the worktree path.",
            )
        })
}

pub(crate) fn refresh_dispatch_record_from_snapshot(
    record: TaskDispatchRecord,
    snapshot: &RemoteDispatchSnapshot,
) -> Result<TaskDispatchRecord, TrackError> {
    if snapshot.is_missing() {
        if let Some(updated) = record
            .clone()
            .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
        {
            return Ok(updated);
        }

        return Ok(record);
    }

    if snapshot.is_running() {
        return Ok(record.mark_running_from_remote(now_utc()));
    }

    if snapshot.is_canceled() {
        let refreshed_at = now_utc();
        let finished_at = snapshot.finished_at_or(refreshed_at);
        return Ok(record.mark_canceled_from_remote(refreshed_at, finished_at));
    }

    let refreshed_at = now_utc();
    let finished_at = snapshot.finished_at_or(refreshed_at);
    if snapshot.is_completed() {
        let remote_result = snapshot
            .required_result("Remote agent run completed without producing a structured result.")?;
        let outcome = ClaudeStructuredOutputEnvelope::<RemoteAgentDispatchOutcome>::parse_result(
            remote_result,
            record.preferred_tool,
            "Remote agent result",
        )?;
        return Ok(record.apply_remote_dispatch_outcome(outcome, refreshed_at, finished_at));
    }

    Ok(record.mark_failed_from_remote_refresh(
        refreshed_at,
        finished_at,
        snapshot.failure_message("Remote agent run failed before returning a structured result."),
    ))
}
