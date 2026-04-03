use std::collections::BTreeSet;

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::now_utc;
use track_types::types::{
    DispatchStatus, RemoteCleanupSummary, RemoteResetSummary, Status, Task, TaskDispatchRecord,
    TaskUpdateInput,
};

use crate::remote_actions::{
    CleanupOrphanedRemoteArtifactsAction, CleanupReviewWorkspaceCachesAction,
    CleanupTaskArtifactsAction, LoadRemoteRegistryAction, ResetWorkspaceAction,
    WriteRemoteRegistryAction,
};
use crate::ssh::SshClient;
use crate::types::{RemoteArtifactCleanupCounts, RemoteTaskCleanupMode};
use crate::utils::{
    describe_remote_reset_blockers, unique_review_run_directories, unique_review_worktree_paths,
};

use super::refresh::derive_remote_run_directory_for_record;
use super::RemoteDispatchService;

impl<'a> RemoteDispatchService<'a> {
    // =============================================================================
    // Task Lifecycle Cleanup
    // =============================================================================
    //
    // Remote agent work leaves behind two different kinds of state:
    //
    // 1. lightweight metadata we want to keep for run history and follow-ups
    // 2. heavyweight worktrees that can accumulate large Rust build outputs
    //
    // Closing a task should therefore keep dispatch history but release the
    // worktree space. Deleting a task goes further and removes both the local
    // dispatch history and the remote run directories as well.
    // We intentionally leave branches and the shared project checkout in
    // place. The heavy cost is in per-task worktrees and their build outputs,
    // while branches and the reusable checkout are comparatively cheap and
    // valuable for follow-up work.
    pub fn update_task(&self, task_id: &str, input: TaskUpdateInput) -> Result<Task, TrackError> {
        let validated_input = input.validate()?;

        if validated_input.status == Some(Status::Closed) {
            let dispatch_history = self.dispatch_repository.dispatches_for_task(task_id)?;
            if !dispatch_history.is_empty() {
                let cleanup_result = self.cleanup_task_remote_artifacts(
                    task_id,
                    &dispatch_history,
                    RemoteTaskCleanupMode::CloseTask,
                );

                // The tracker should stay usable even if the remote machine,
                // SSH key, or remote config disappears. Closing the task is a
                // local filesystem mutation first; remote cleanup is only a
                // best-effort follow-up.
                match cleanup_result {
                    Ok(_) => self.finalize_active_dispatches_locally(
                        &dispatch_history,
                        DispatchStatus::Canceled,
                        "Canceled because the task was closed.",
                        None,
                    )?,
                    Err(error) => {
                        eprintln!("Skipping remote cleanup while closing task {task_id}: {error}");
                        self.finalize_active_dispatches_locally(
                            &dispatch_history,
                            DispatchStatus::Canceled,
                            "Canceled because the task was closed locally. Remote cleanup was skipped.",
                            Some(error.message()),
                        )?;
                    }
                }
            }
        }

        self.task_repository.update_task(task_id, validated_input)
    }

    pub fn delete_task(&self, task_id: &str) -> Result<(), TrackError> {
        let dispatch_history = self.dispatch_repository.dispatches_for_task(task_id)?;
        if !dispatch_history.is_empty() {
            if let Err(error) = self.cleanup_task_remote_artifacts(
                task_id,
                &dispatch_history,
                RemoteTaskCleanupMode::DeleteTask,
            ) {
                // Delete is the strongest local intent: once the user removes a
                // task, stale remote artifacts must not veto that choice.
                // TODO: We intentionally do not persist this warning locally
                // because delete also removes the task's local dispatch history.
                eprintln!("Skipping remote cleanup while deleting task {task_id}: {error}");
            }

            // We intentionally remove the local dispatch history before the
            // task file itself. If the final file delete fails, the user still
            // sees the task and can retry, rather than ending up with invisible
            // orphaned runs in the UI.
            self.dispatch_repository
                .delete_dispatch_history_for_task(task_id)?;
        }

        self.task_repository.delete_task(task_id)
    }

    // =============================================================================
    // Manual Remote Cleanup
    // =============================================================================
    //
    // The lifecycle hooks on close/delete protect new work from leaking
    // worktrees forever, but users may already have historical leftovers from
    // before that policy existed. Manual cleanup replays the same rules across
    // the whole tracker state:
    //
    // - open task: keep referenced worktrees and dispatch metadata
    // - closed task: remove worktrees, keep dispatch metadata
    // - persisted review: keep local review history, but only keep remote
    //   artifacts while the review run is still active
    // - missing task/review: remove remote artifacts and local dispatch history
    //
    // After reconciling every saved history, we also sweep the remote
    // workspace for orphaned task/review worktrees and run directories that no
    // longer have any local record at all. Review-only checkouts are also
    // treated as disposable cache during this explicit maintenance path
    // because reviews currently do not support rerun or resume from an old
    // worktree.
    // TODO: Revisit review checkout cleanup if manual reviews ever gain
    // rerun/reopen behavior that should preserve those remote caches.
    pub fn cleanup_unused_remote_artifacts(&self) -> Result<RemoteCleanupSummary, TrackError> {
        let remote_agent = self.load_remote_agent_for_global_cleanup()?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let task_ids_with_history = self.dispatch_repository.task_ids_with_history()?;
        let review_ids_with_history = self.review_dispatch_repository.review_ids_with_history()?;
        let tracked_project_names = self
            .project_repository
            .list_projects()?
            .into_iter()
            .map(|project| project.canonical_name)
            .collect::<BTreeSet<_>>();

        let mut summary = RemoteCleanupSummary::default();
        let mut kept_worktree_paths = BTreeSet::new();
        let mut kept_run_directories = BTreeSet::new();
        let mut review_workspace_keys = BTreeSet::new();
        let mut active_review_workspace_keys = BTreeSet::new();

        for task_id in task_ids_with_history {
            let dispatch_history = self.dispatch_repository.dispatches_for_task(&task_id)?;
            if dispatch_history.is_empty() {
                continue;
            }

            match self.task_repository.get_task(&task_id) {
                Ok(task) if task.status == Status::Open => {
                    kept_worktree_paths.extend(unique_worktree_paths(&dispatch_history));
                    kept_run_directories
                        .extend(unique_run_directories(&dispatch_history, &remote_agent));
                }
                Ok(task) if task.status == Status::Closed => {
                    let cleanup_counts = self.cleanup_task_remote_artifacts(
                        &task.id,
                        &dispatch_history,
                        RemoteTaskCleanupMode::CloseTask,
                    )?;
                    self.finalize_active_dispatches_locally(
                        &dispatch_history,
                        DispatchStatus::Canceled,
                        "Canceled because the task was closed.",
                        None,
                    )?;
                    kept_run_directories
                        .extend(unique_run_directories(&dispatch_history, &remote_agent));
                    summary.closed_tasks_cleaned += 1;
                    summary.remote_worktrees_removed += cleanup_counts.worktrees_removed;
                    summary.remote_run_directories_removed +=
                        cleanup_counts.run_directories_removed;
                }
                Err(error) if error.code == ErrorCode::TaskNotFound => {
                    let cleanup_counts = self.cleanup_task_remote_artifacts(
                        &task_id,
                        &dispatch_history,
                        RemoteTaskCleanupMode::DeleteTask,
                    )?;
                    self.dispatch_repository
                        .delete_dispatch_history_for_task(&task_id)?;
                    summary.missing_tasks_cleaned += 1;
                    summary.local_dispatch_histories_removed += 1;
                    summary.remote_worktrees_removed += cleanup_counts.worktrees_removed;
                    summary.remote_run_directories_removed +=
                        cleanup_counts.run_directories_removed;
                }
                Err(error) => return Err(error),
                Ok(_) => unreachable!("tasks should only be open or closed"),
            }
        }

        for review_id in review_ids_with_history {
            let dispatch_history = self
                .review_dispatch_repository
                .dispatches_for_review(&review_id)?;
            if dispatch_history.is_empty() {
                continue;
            }

            let workspace_key = dispatch_history[0].workspace_key.clone();
            review_workspace_keys.insert(workspace_key.clone());

            match self.review_repository.get_review(&review_id) {
                Ok(_) => {
                    let active_dispatch_history = dispatch_history
                        .iter()
                        .filter(|record| record.status.is_active())
                        .cloned()
                        .collect::<Vec<_>>();
                    if !active_dispatch_history.is_empty() {
                        kept_worktree_paths
                            .extend(unique_review_worktree_paths(&active_dispatch_history));
                        kept_run_directories.extend(unique_review_run_directories(
                            &active_dispatch_history,
                            &remote_agent,
                        ));
                        active_review_workspace_keys.insert(workspace_key);
                    }
                }
                Err(error) if error.code == ErrorCode::TaskNotFound => {
                    self.review_dispatch_repository
                        .delete_dispatch_history_for_review(&review_id)?;
                    summary.local_dispatch_histories_removed += 1;
                }
                Err(error) => return Err(error),
            }
        }

        let orphan_cleanup_counts = CleanupOrphanedRemoteArtifactsAction::new(
            &ssh_client,
            &remote_agent.workspace_root,
            &kept_worktree_paths.into_iter().collect::<Vec<_>>(),
            &kept_run_directories.into_iter().collect::<Vec<_>>(),
        )
        .execute()?;
        summary.remote_worktrees_removed += orphan_cleanup_counts.worktrees_removed;
        summary.remote_run_directories_removed += orphan_cleanup_counts.run_directories_removed;

        let reclaimable_review_workspace_keys = review_workspace_keys
            .into_iter()
            .filter(|workspace_key| {
                !tracked_project_names.contains(workspace_key)
                    && !active_review_workspace_keys.contains(workspace_key)
            })
            .collect::<Vec<_>>();
        self.cleanup_reclaimable_review_workspaces(
            &ssh_client,
            &remote_agent,
            &reclaimable_review_workspace_keys,
        )?;

        Ok(summary)
    }

    // =============================================================================
    // Full Remote Workspace Reset
    // =============================================================================
    //
    // Manual cleanup reconciles remote state against local truth. Reset is the
    // explicit escape hatch for the harder case where the remote machine is no
    // longer trustworthy and the user wants to start that environment from
    // scratch.
    //
    // We intentionally keep local task files and local dispatch history.
    // Those remain the durable tracker state. The remote workspace root and
    // the remote projects registry are treated as rebuildable cache.
    pub fn reset_remote_workspace(&self) -> Result<RemoteResetSummary, TrackError> {
        let active_task_dispatches = self.list_dispatches(None)?;
        let active_review_dispatches = self.review_service().list_dispatches(None)?;
        let active_dispatches =
            describe_remote_reset_blockers(&active_task_dispatches, &active_review_dispatches);
        if !active_dispatches.is_empty() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Stop active remote task runs and PR reviews before resetting the remote workspace: {}.",
                    active_dispatches.join(", ")
                ),
            ));
        }

        let remote_agent = self.load_remote_agent_for_global_cleanup()?;
        let ssh_client = SshClient::new(&remote_agent)?;
        ResetWorkspaceAction::new(
            &ssh_client,
            &remote_agent.workspace_root,
            &remote_agent.projects_registry_path,
        )
        .execute()
    }

    // =============================================================================
    // Local Recovery When Remote Control Disappears
    // =============================================================================
    //
    // Remote agent runs are helpful, but they are not the source of truth. The
    // source of truth is still the local tracker on disk, and users need to be
    // able to keep closing, deleting, and retrying tasks even after the remote
    // machine has been replaced or the SSH setup has gone stale. These helpers
    // therefore turn "we can no longer inspect or clean the remote side" into
    // explicit local terminal records instead of leaving active dispatches stuck
    // forever.
    fn cleanup_task_remote_artifacts(
        &self,
        task_id: &str,
        dispatch_history: &[TaskDispatchRecord],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        if dispatch_history.is_empty() {
            return Ok(RemoteArtifactCleanupCounts::default());
        }

        let remote_agent = self.load_remote_agent_for_cleanup(task_id)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let checkout_path = self.resolve_project_checkout_path(
            &ssh_client,
            &remote_agent,
            &dispatch_history[0].project,
        )?;
        let worktree_paths = unique_worktree_paths(dispatch_history);
        let run_directories = unique_run_directories(dispatch_history, &remote_agent);

        CleanupTaskArtifactsAction::new(
            &ssh_client,
            &checkout_path,
            &worktree_paths,
            &run_directories,
            cleanup_mode,
        )
        .execute()
    }

    fn finalize_active_dispatches_locally(
        &self,
        dispatch_history: &[TaskDispatchRecord],
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<(), TrackError> {
        for dispatch_record in dispatch_history {
            if !dispatch_record.status.is_active() {
                continue;
            }

            self.finalize_dispatch_locally(dispatch_record, status, summary, error_message)?;
        }

        Ok(())
    }

    pub(super) fn finalize_dispatch_locally(
        &self,
        dispatch_record: &TaskDispatchRecord,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let mut updated_record = dispatch_record.clone();
        let now = now_utc();
        updated_record.status = status;
        updated_record.updated_at = now;
        updated_record.finished_at = Some(now);
        updated_record.summary = Some(summary.to_owned());
        updated_record.notes = None;
        updated_record.error_message = error_message.map(ToOwned::to_owned);
        self.dispatch_repository.save_dispatch(&updated_record)?;

        Ok(updated_record)
    }

    fn load_remote_agent_for_cleanup(
        &self,
        task_id: &str,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    format!(
                        "Task {task_id} has remote dispatch history, but remote-agent configuration is missing so cleanup cannot run."
                    ),
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before cleaning task {task_id}.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        Ok(remote_agent)
    }

    fn load_remote_agent_for_global_cleanup(&self) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    "Remote cleanup cannot run because remote-agent configuration is missing.",
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before running cleanup.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        Ok(remote_agent)
    }

    fn cleanup_reclaimable_review_workspaces(
        &self,
        ssh_client: &SshClient,
        remote_agent: &RemoteAgentRuntimeConfig,
        workspace_keys: &[String],
    ) -> Result<(), TrackError> {
        if workspace_keys.is_empty() {
            return Ok(());
        }

        let mut remote_registry =
            LoadRemoteRegistryAction::new(ssh_client, &remote_agent.projects_registry_path)
                .execute()?;
        let checkout_paths = workspace_keys
            .iter()
            .map(|workspace_key| {
                remote_registry
                    .projects
                    .get(workspace_key)
                    .map(|entry| entry.checkout_path.clone())
                    .unwrap_or_else(|| {
                        format!(
                            "{}/{}/{}",
                            remote_agent.workspace_root.trim_end_matches('/'),
                            workspace_key,
                            workspace_key
                        )
                    })
            })
            .collect::<Vec<_>>();

        CleanupReviewWorkspaceCachesAction::new(ssh_client, &checkout_paths).execute()?;

        let mut registry_changed = false;
        for workspace_key in workspace_keys {
            registry_changed |= remote_registry.projects.remove(workspace_key).is_some();
        }

        if registry_changed {
            WriteRemoteRegistryAction::new(
                ssh_client,
                &remote_agent.projects_registry_path,
                &remote_registry,
            )
            .execute()?;
        }

        Ok(())
    }

    fn resolve_project_checkout_path(
        &self,
        ssh_client: &SshClient,
        remote_agent: &RemoteAgentRuntimeConfig,
        project_name: &str,
    ) -> Result<String, TrackError> {
        let remote_registry =
            LoadRemoteRegistryAction::new(ssh_client, &remote_agent.projects_registry_path)
                .execute()?;

        Ok(remote_registry
            .projects
            .get(project_name)
            .map(|entry| entry.checkout_path.clone())
            .unwrap_or_else(|| {
                format!(
                    "{}/{}/{}",
                    remote_agent.workspace_root.trim_end_matches('/'),
                    project_name,
                    project_name
                )
            }))
    }
}

fn unique_worktree_paths(dispatch_history: &[TaskDispatchRecord]) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| record.worktree_path.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn unique_run_directories(
    dispatch_history: &[TaskDispatchRecord],
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| derive_remote_run_directory_for_record(record, remote_agent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
