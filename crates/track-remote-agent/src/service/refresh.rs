use std::collections::BTreeMap;

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::{now_utc, parse_iso_8601_seconds};
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

use super::{RemoteDispatchService, RemoteReviewService};

impl<'a> RemoteDispatchService<'a> {
    pub(super) fn refresh_active_dispatch_records(
        &self,
        records: Vec<TaskDispatchRecord>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let remote_agent = match self.config_service.load_remote_agent_runtime_config() {
            Ok(config) => config,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::ConfigNotFound
                        | ErrorCode::InvalidConfig
                        | ErrorCode::InvalidRemoteAgentConfig
                ) =>
            {
                let error_message = error.to_string();
                return self.release_active_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation is unavailable locally, so active runs were released.",
                    &error_message,
                );
            }
            Err(error) => return Err(error),
        };

        let Some(remote_agent) = remote_agent else {
            return self.release_active_dispatches_after_reconciliation_loss(
                records,
                "Remote reconciliation is unavailable locally, so active runs were released.",
                "Remote agent configuration is missing locally.",
            );
        };
        if !remote_agent.managed_key_path.exists() {
            let error_message = format!(
                "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                collapse_home_path(&remote_agent.managed_key_path)
            );
            return self.release_active_dispatches_after_reconciliation_loss(
                records,
                "Remote reconciliation is unavailable locally, so active runs were released.",
                &error_message,
            );
        }

        let ssh_client = SshClient::new(&remote_agent)?;
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
                if let Some(updated) = mark_abandoned_preparing_dispatch(record.clone()) {
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
                    if let Some(updated) =
                        mark_terminal_refresh_failure(record.clone(), snapshot, &error)
                    {
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
        let remote_agent = match self.config_service.load_remote_agent_runtime_config() {
            Ok(config) => config,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::ConfigNotFound
                        | ErrorCode::InvalidConfig
                        | ErrorCode::InvalidRemoteAgentConfig
                ) =>
            {
                let error_message = error.to_string();
                return self.release_active_review_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation is unavailable locally, so active review runs were released.",
                    &error_message,
                );
            }
            Err(error) => return Err(error),
        };

        let Some(remote_agent) = remote_agent else {
            return self.release_active_review_dispatches_after_reconciliation_loss(
                records,
                "Remote reconciliation is unavailable locally, so active review runs were released.",
                "Remote agent configuration is missing locally.",
            );
        };
        if !remote_agent.managed_key_path.exists() {
            let error_message = format!(
                "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                collapse_home_path(&remote_agent.managed_key_path)
            );
            return self.release_active_review_dispatches_after_reconciliation_loss(
                records,
                "Remote reconciliation is unavailable locally, so active review runs were released.",
                &error_message,
            );
        }

        let ssh_client = SshClient::new(&remote_agent)?;
        let snapshots_by_dispatch_id = load_review_snapshots_for_records(&ssh_client, &records)?;
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(&record.dispatch_id) else {
                if let Some(updated) = mark_abandoned_preparing_review_dispatch(record.clone()) {
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
                    if let Some(updated) =
                        mark_terminal_review_refresh_failure(record.clone(), snapshot, &error)
                    {
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
        mut record: ReviewRunRecord,
        snapshot: &RemoteDispatchSnapshot,
    ) -> Result<ReviewRunRecord, TrackError> {
        let remote_status = snapshot.status.as_deref().unwrap_or_default().trim();
        if remote_status.is_empty() {
            if let Some(updated) = mark_abandoned_preparing_review_dispatch(record.clone()) {
                return Ok(updated);
            }

            return Ok(record);
        }

        if remote_status == "running" {
            if record.status == DispatchStatus::Preparing {
                record.status = DispatchStatus::Running;
                record.updated_at = now_utc();
                record.finished_at = None;
                record.error_message = None;
            }
            return Ok(record);
        }

        if remote_status == "canceled" {
            record.status = DispatchStatus::Canceled;
            record.updated_at = now_utc();
            record.finished_at = Some(parse_remote_finished_at(
                snapshot.finished_at.as_deref(),
                now_utc(),
            ));
            record.summary = Some(
                record
                    .summary
                    .unwrap_or_else(|| "Canceled from the web UI.".to_owned()),
            );
            record.error_message = None;
            return Ok(record);
        }

        let now = now_utc();
        record.updated_at = now;
        if remote_status == "completed" {
            let remote_result = snapshot.result.as_deref().ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    "Remote review run completed without producing a structured result.",
                )
            })?;
            let outcome = ClaudeStructuredOutputEnvelope::<RemoteAgentReviewOutcome>::parse_result(
                remote_result,
                record.preferred_tool,
                "Remote review result",
            )?;

            record.status = outcome.status;
            record.summary = Some(outcome.summary);
            record.review_submitted = outcome.review_submitted;
            record.github_review_id = outcome.github_review_id;
            record.github_review_url = outcome.github_review_url;
            record.worktree_path = Some(outcome.worktree_path);
            record.notes = outcome.notes;
            record.error_message = None;
            record.finished_at = Some(parse_remote_finished_at(
                snapshot.finished_at.as_deref(),
                now,
            ));

            return Ok(record);
        }

        record.status = DispatchStatus::Failed;
        record.finished_at = Some(parse_remote_finished_at(
            snapshot.finished_at.as_deref(),
            now,
        ));
        record.error_message = snapshot
            .stderr
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
            .or_else(|| {
                Some("Remote review run failed before returning a structured result.".to_owned())
            });
        Ok(record)
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
    mut record: TaskDispatchRecord,
    snapshot: &RemoteDispatchSnapshot,
) -> Result<TaskDispatchRecord, TrackError> {
    let remote_status = snapshot.status.as_deref().unwrap_or_default().trim();
    if remote_status.is_empty() {
        if let Some(updated) = mark_abandoned_preparing_dispatch(record.clone()) {
            return Ok(updated);
        }

        return Ok(record);
    }

    if remote_status == "running" {
        if record.status == DispatchStatus::Preparing {
            record.status = DispatchStatus::Running;
            record.updated_at = now_utc();
            record.finished_at = None;
            record.error_message = None;
        }
        return Ok(record);
    }

    if remote_status == "canceled" {
        record.status = DispatchStatus::Canceled;
        record.updated_at = now_utc();
        record.finished_at = Some(parse_remote_finished_at(
            snapshot.finished_at.as_deref(),
            now_utc(),
        ));
        record.summary = Some(
            record
                .summary
                .unwrap_or_else(|| "Canceled from the web UI.".to_owned()),
        );
        record.error_message = None;
        return Ok(record);
    }

    let now = now_utc();
    record.updated_at = now;
    if remote_status == "completed" {
        let remote_result = snapshot.result.as_deref().ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Remote agent run completed without producing a structured result.",
            )
        })?;
        let outcome = ClaudeStructuredOutputEnvelope::<RemoteAgentDispatchOutcome>::parse_result(
            remote_result,
            record.preferred_tool,
            "Remote agent result",
        )?;
        record.status = outcome.status;
        record.summary = Some(outcome.summary);
        record.pull_request_url = outcome.pull_request_url;
        record.branch_name = outcome.branch_name.or(record.branch_name);
        record.worktree_path = Some(outcome.worktree_path);
        record.notes = outcome.notes;
        record.error_message = None;
        record.finished_at = Some(parse_remote_finished_at(
            snapshot.finished_at.as_deref(),
            now,
        ));
        return Ok(record);
    }

    record.status = DispatchStatus::Failed;
    record.finished_at = Some(parse_remote_finished_at(
        snapshot.finished_at.as_deref(),
        now,
    ));
    record.error_message = snapshot
        .stderr
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_owned())
        .or_else(|| {
            Some("Remote agent run failed before returning a structured result.".to_owned())
        });
    Ok(record)
}

pub(crate) fn mark_abandoned_preparing_dispatch(
    mut record: TaskDispatchRecord,
) -> Option<TaskDispatchRecord> {
    if record.status != DispatchStatus::Preparing {
        return None;
    }

    let now = now_utc();
    if now - record.updated_at < PREPARING_STALE_AFTER {
        return None;
    }

    record.status = DispatchStatus::Failed;
    record.updated_at = now;
    record.finished_at = Some(now);
    record.error_message =
        Some("Dispatch preparation stopped before the remote agent launched.".to_owned());
    Some(record)
}

pub(crate) fn mark_abandoned_preparing_review_dispatch(
    mut record: ReviewRunRecord,
) -> Option<ReviewRunRecord> {
    if record.status != DispatchStatus::Preparing {
        return None;
    }

    let now = now_utc();
    if now - record.updated_at < PREPARING_STALE_AFTER {
        return None;
    }

    record.status = DispatchStatus::Failed;
    record.updated_at = now;
    record.finished_at = Some(now);
    record.error_message =
        Some("Review preparation stopped before the remote agent launched.".to_owned());
    Some(record)
}

pub(crate) fn parse_remote_finished_at(
    value: Option<&str>,
    fallback: time::OffsetDateTime,
) -> time::OffsetDateTime {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| parse_iso_8601_seconds(value).ok())
        .unwrap_or(fallback)
}

pub(crate) fn mark_terminal_refresh_failure(
    mut record: TaskDispatchRecord,
    snapshot: &RemoteDispatchSnapshot,
    error: &TrackError,
) -> Option<TaskDispatchRecord> {
    let remote_status = snapshot.status.as_deref().unwrap_or_default().trim();
    if remote_status != "completed" && remote_status != "launcher_failed" {
        return None;
    }

    let now = now_utc();
    record.status = DispatchStatus::Failed;
    record.updated_at = now;
    record.finished_at = Some(parse_remote_finished_at(
        snapshot.finished_at.as_deref(),
        now,
    ));
    record.error_message = Some(error.to_string());
    Some(record)
}

pub(crate) fn mark_terminal_review_refresh_failure(
    mut record: ReviewRunRecord,
    snapshot: &RemoteDispatchSnapshot,
    error: &TrackError,
) -> Option<ReviewRunRecord> {
    let remote_status = snapshot.status.as_deref().unwrap_or_default().trim();
    if remote_status != "completed" && remote_status != "launcher_failed" {
        return None;
    }

    let now = now_utc();
    record.status = DispatchStatus::Failed;
    record.updated_at = now;
    record.finished_at = Some(parse_remote_finished_at(
        snapshot.finished_at.as_deref(),
        now,
    ));
    record.error_message = Some(error.to_string());
    Some(record)
}
