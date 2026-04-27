use std::collections::BTreeSet;
use std::sync::Arc;

use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::task_repository::FileTaskRepository;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::{DispatchId, TaskId};
use track_types::remote_layout::{DispatchBranch, DispatchRunDirectory, DispatchWorktreePath};
use track_types::task_description::append_follow_up_request;
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{
    DispatchStatus, RemoteAgentPreferredTool, Status, Task, TaskDispatchRecord, TaskUpdateInput,
};
use track_types::urls::Url;

use crate::types::{RemoteArtifactCleanupCounts, RemoteTaskCleanupMode};
use crate::utils::parse_github_repository_name;
use crate::RemoteTaskArtifactCleanupMode;
use crate::RemoteWorkspace;

pub(crate) use self::guard::TaskDispatchStartGuard;
use self::record_ext::first_follow_up_line;
#[cfg(test)]
pub(super) use self::refresh::refresh_dispatch_record_from_snapshot;

mod cancel;
mod guard;
mod launch;
mod record_ext;
mod refresh;

pub struct RemoteDispatchService<'a> {
    pub(super) database: &'a DatabaseContext,
    pub(super) workspace: Arc<RemoteWorkspace>,
}

impl<'a> RemoteDispatchService<'a> {
    fn dispatch_repository(&self) -> DispatchRepository<'a> {
        self.database.dispatch_repository()
    }

    fn project_repository(&self) -> ProjectRepository<'a> {
        self.database.project_repository()
    }

    fn task_repository(&self) -> FileTaskRepository<'a> {
        self.database.task_repository()
    }

    // =============================================================================
    // Remote Dispatch Entry Points
    // =============================================================================
    //
    // This file keeps the task-dispatch entry points in one readable story:
    // queueing, follow-up reuse, launch, refresh, and cleanup. Lifecycle
    // adapter code lives in sibling modules with matching names.
    #[tracing::instrument(skip(self), fields(task_id = %task_id, preferred_tool = ?preferred_tool))]
    pub async fn queue_dispatch(
        &self,
        task_id: &TaskId,
        preferred_tool: Option<RemoteAgentPreferredTool>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let (task, _project_metadata) = self.load_dispatch_prerequisites(task_id).await?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id).await?;
        let remote_agent = self.workspace.remote_agent();
        let preferred_tool = preferred_tool.unwrap_or(remote_agent.preferred_tool);
        let dispatch_id = DispatchId::unique();
        let branch_name = DispatchBranch::for_task(&dispatch_id);
        let worktree_path = DispatchWorktreePath::for_task(
            &remote_agent.workspace_root,
            &task.project,
            &dispatch_id,
        );

        let dispatch_record = self
            .dispatch_repository()
            .create_dispatch(
                &task,
                &dispatch_id,
                &remote_agent.host,
                preferred_tool,
                &branch_name,
                &worktree_path,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;

        tracing::info!(
            dispatch_id = %dispatch_record.run.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.run.remote_host,
            branch_name = ?branch_name,
            worktree_path = ?worktree_path,
            preferred_tool = ?dispatch_record.run.preferred_tool,
            "Queued remote task dispatch"
        );

        Ok(dispatch_record)
    }

    // =============================================================================
    // Follow-Up Dispatches
    // =============================================================================
    //
    // A follow-up continues an earlier remote attempt instead of starting from
    // scratch. We keep the previous branch/worktree/PR context, append the new
    // user request into the task Markdown for auditability, and store the
    // latest follow-up request directly on the dispatch record so prompt
    // generation can highlight the newest ask explicitly.
    #[tracing::instrument(skip(self, follow_up_request), fields(task_id = %task_id))]
    pub async fn queue_follow_up_dispatch(
        &self,
        task_id: &TaskId,
        follow_up_request: &str,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let trimmed_follow_up_request = follow_up_request.trim();
        if trimmed_follow_up_request.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a follow-up request for the remote agent.",
            ));
        }

        let (_task, _project_metadata) = self.load_dispatch_prerequisites(task_id).await?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id).await?;
        let remote_agent = self.workspace.remote_agent();

        let dispatch_history = self
            .dispatch_repository()
            .dispatches_for_task(task_id)
            .await?;
        let previous_dispatch = select_follow_up_base_dispatch(task_id, &dispatch_history)?;
        let branch_name = previous_dispatch.run.branch_name.clone().ok_or_else(|| {
            dispatch_not_found(
                task_id,
                "does not have a reusable branch from the previous remote dispatch.",
            )
        })?;
        let worktree_path = previous_dispatch.run.worktree_path.clone().ok_or_else(|| {
            dispatch_not_found(
                task_id,
                "does not have a reusable worktree from the previous remote dispatch.",
            )
        })?;

        let updated_task = self
            .append_follow_up_request_to_task(task_id, trimmed_follow_up_request)
            .await?;
        let pull_request_url = latest_pull_request_for_branch(&dispatch_history, &branch_name)
            .or(previous_dispatch.pull_request_url.clone());
        let dispatch_id = DispatchId::unique();
        let summary = format!(
            "Follow-up request: {}",
            first_follow_up_line(trimmed_follow_up_request)
        );
        let dispatch_record = self
            .dispatch_repository()
            .create_dispatch(
                &updated_task,
                &dispatch_id,
                &remote_agent.host,
                previous_dispatch.run.preferred_tool,
                &branch_name,
                &worktree_path,
                pull_request_url.as_ref(),
                Some(trimmed_follow_up_request),
                Some(summary.as_str()),
                previous_dispatch.review_request_head_oid.as_deref(),
                previous_dispatch.review_request_user.as_deref(),
            )
            .await?;

        tracing::info!(
            dispatch_id = %dispatch_record.run.dispatch_id,
            base_dispatch_id = %previous_dispatch.run.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.run.remote_host,
            branch_name = ?branch_name,
            worktree_path = ?worktree_path,
            preferred_tool = ?dispatch_record.run.preferred_tool,
            follow_up_lines = trimmed_follow_up_request.lines().count(),
            "Queued remote task follow-up dispatch"
        );

        Ok(dispatch_record)
    }

    #[tracing::instrument(
        skip(self, dispatch_record),
        fields(
            task_id = %dispatch_record.task_id,
            dispatch_id = %dispatch_record.run.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.run.remote_host,
            preferred_tool = ?dispatch_record.run.preferred_tool
        )
    )]
    pub async fn launch_prepared_dispatch(
        &self,
        dispatch_record: TaskDispatchRecord,
    ) -> Result<TaskDispatchRecord, TrackError> {
        launch::launch_prepared_dispatch(self, dispatch_record).await
    }

    // =============================================================================
    // Dispatch Cancellation
    // =============================================================================
    //
    // Cancellation means "stop spending remote resources on this run". The
    // runtime service owns it because it may need to signal the remote helper
    // before finalizing the local dispatch record.
    #[tracing::instrument(skip(self), fields(task_id = %task_id))]
    pub async fn cancel_dispatch(
        &self,
        task_id: &TaskId,
    ) -> Result<TaskDispatchRecord, TrackError> {
        cancel::cancel_dispatch(self, task_id).await
    }

    pub async fn latest_dispatches_for_tasks(
        &self,
        task_ids: &[TaskId],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self
            .dispatch_repository()
            .latest_dispatches_for_tasks(task_ids)
            .await?;
        self.refresh_active_dispatch_records(records).await
    }

    // =============================================================================
    // Runtime Dispatch History
    // =============================================================================
    //
    // Runtime flows such as workspace reset need the same "what is the remote
    // machine saying right now?" view as queueing and cancellation. We keep
    // that refresh here so callers with a runtime service see active records
    // reconciled before making remote-lifecycle decisions.
    pub async fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self.dispatch_repository().list_dispatches(limit).await?;
        self.refresh_active_dispatch_records(records).await
    }

    // =============================================================================
    // Task Lifecycle Cleanup
    // =============================================================================
    //
    // The dispatch domain owns task close/delete cleanup because those actions
    // are really about reclaiming task-specific remote worktrees while keeping
    // the local tracker usable if the remote side has already gone away.
    #[tracing::instrument(skip(self, input), fields(task_id = %task_id))]
    pub async fn update_task(
        &self,
        task_id: &TaskId,
        input: TaskUpdateInput,
    ) -> Result<Task, TrackError> {
        let validated_input = input.validate()?;

        if validated_input.status == Some(Status::Closed) {
            let dispatch_history = self
                .dispatch_repository()
                .dispatches_for_task(task_id)
                .await?;
            if !dispatch_history.is_empty() {
                let cleanup_result = self
                    .cleanup_task_remote_artifacts(
                        task_id,
                        &dispatch_history,
                        RemoteTaskCleanupMode::CloseTask,
                    )
                    .await;

                // The tracker should stay usable if the remote machine itself
                // is unhealthy. Local remote-agent configuration is different:
                // by this point the API should have kept it stable, so missing
                // config or SSH material is surfaced as a local invariant
                // failure instead of being treated as a cleanup detail.
                match cleanup_result {
                    Ok(_) => {
                        tracing::info!("Cleaned remote task artifacts while closing task");
                        self.finalize_active_dispatches_locally(
                            &dispatch_history,
                            DispatchStatus::Canceled,
                            "Canceled because the task was closed.",
                            None,
                        )
                        .await?
                    }
                    Err(error) if remote_cleanup_can_be_skipped(&error) => {
                        tracing::warn!(error = %error, "Skipping remote cleanup while closing task");
                        self.finalize_active_dispatches_locally(
                            &dispatch_history,
                            DispatchStatus::Canceled,
                            "Canceled because the task was closed locally. Remote cleanup was skipped.",
                            Some(error.message()),
                        )
                        .await?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }

        let updated_task = self
            .task_repository()
            .update_task(task_id, validated_input)
            .await?;
        tracing::info!(status = ?updated_task.status, project = %updated_task.project, "Updated task");
        Ok(updated_task)
    }

    #[tracing::instrument(skip(self), fields(task_id = %task_id))]
    pub async fn delete_task(&self, task_id: &TaskId) -> Result<(), TrackError> {
        let dispatch_history = self
            .dispatch_repository()
            .dispatches_for_task(task_id)
            .await?;
        if !dispatch_history.is_empty() {
            if let Err(error) = self
                .cleanup_task_remote_artifacts(
                    task_id,
                    &dispatch_history,
                    RemoteTaskCleanupMode::DeleteTask,
                )
                .await
            {
                if !remote_cleanup_can_be_skipped(&error) {
                    return Err(error);
                }

                // Delete is the strongest local intent: once the user removes a
                // task, stale artifacts on an unhealthy remote machine must not
                // veto that choice.
                // TODO: We intentionally do not persist this warning locally
                // because delete also removes the task's local dispatch history.
                tracing::warn!(error = %error, "Skipping remote cleanup while deleting task");
            }

            // We intentionally remove the local dispatch history before the
            // task file itself. If the final file delete fails, the user still
            // sees the task and can retry, rather than ending up with invisible
            // orphaned runs in the UI.
            self.dispatch_repository()
                .delete_dispatch_history_for_task(task_id)
                .await?;
        }

        self.task_repository().delete_task(task_id).await?;
        tracing::info!("Deleted task and any local dispatch history");
        Ok(())
    }

    pub async fn refresh_active_dispatch_records(
        &self,
        records: Vec<TaskDispatchRecord>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        refresh::refresh_active_dispatch_records(self, records).await
    }

    pub(super) async fn cleanup_task_remote_artifacts(
        &self,
        task_id: &TaskId,
        dispatch_history: &[TaskDispatchRecord],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        if dispatch_history.is_empty() {
            return Ok(RemoteArtifactCleanupCounts::default());
        }

        let checkout_path = self
            .workspace
            .projects()
            .resolve_checkout_path_for_project(&dispatch_history[0].project);
        let summary = self
            .workspace
            .task_runs()
            .cleanup(
                &checkout_path,
                dispatch_history,
                match cleanup_mode {
                    RemoteTaskCleanupMode::CloseTask => RemoteTaskArtifactCleanupMode::CloseTask,
                    RemoteTaskCleanupMode::DeleteTask => RemoteTaskArtifactCleanupMode::DeleteTask,
                },
            )
            .await?;

        let cleanup_counts = RemoteArtifactCleanupCounts {
            worktrees_removed: summary.worktrees_removed,
            run_directories_removed: summary.run_directories_removed,
        };
        tracing::info!(
            task_id = %task_id,
            cleanup_mode = ?cleanup_mode,
            worktrees_removed = cleanup_counts.worktrees_removed,
            run_directories_removed = cleanup_counts.run_directories_removed,
            "Cleaned remote task artifacts"
        );

        Ok(cleanup_counts)
    }

    pub(super) async fn finalize_active_dispatches_locally(
        &self,
        dispatch_history: &[TaskDispatchRecord],
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<(), TrackError> {
        for dispatch_record in dispatch_history {
            if !dispatch_record.run.status.is_active() {
                continue;
            }

            self.finalize_dispatch_locally(dispatch_record, status, summary, error_message)
                .await?;
        }

        Ok(())
    }

    async fn finalize_dispatch_locally(
        &self,
        dispatch_record: &TaskDispatchRecord,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let updated_record =
            dispatch_record
                .clone()
                .into_locally_finalized(status, summary, error_message);
        self.dispatch_repository()
            .save_dispatch(&updated_record)
            .await?;
        if matches!(status, DispatchStatus::Blocked | DispatchStatus::Failed) {
            tracing::warn!(
                dispatch_id = %updated_record.run.dispatch_id,
                status = ?updated_record.run.status,
                summary = %summary,
                error_message = error_message.unwrap_or(""),
                "Locally finalized task dispatch after remote disruption"
            );
        } else {
            tracing::info!(
                dispatch_id = %updated_record.run.dispatch_id,
                status = ?updated_record.run.status,
                summary = %summary,
                "Locally finalized task dispatch"
            );
        }

        Ok(updated_record)
    }

    async fn ensure_no_blocking_active_dispatch(&self, task_id: &TaskId) -> Result<(), TrackError> {
        if let Some(existing_dispatch) = self
            .latest_dispatches_for_tasks(std::slice::from_ref(task_id))
            .await?
            .into_iter()
            .next()
            .filter(|record| record.run.status.is_active())
        {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Task {task_id} already has an active remote dispatch ({})",
                    existing_dispatch.run.dispatch_id
                ),
            ));
        }

        Ok(())
    }

    async fn cancel_remote_dispatch_if_possible(
        &self,
        dispatch_record: &TaskDispatchRecord,
    ) -> Result<(), TrackError> {
        let Some(worktree_path) = dispatch_record.run.worktree_path.as_ref() else {
            return Ok(());
        };
        let _ = worktree_path;
        self.workspace
            .task_runs()
            .cancel(dispatch_record)
            .await
            .map(|_| ())?;
        tracing::info!(
            dispatch_id = %dispatch_record.run.dispatch_id,
            "Issued remote cancellation for task dispatch"
        );
        Ok(())
    }

    async fn save_preparing_phase(
        &self,
        dispatch_record: &mut TaskDispatchRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) = self
            .dispatch_repository()
            .get_dispatch(&dispatch_record.task_id, &dispatch_record.run.dispatch_id)
            .await?
        {
            if !saved_record.run.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
        } else {
            // If a user discards or deletes the local run while the background
            // launcher is preparing the remote workspace, the launch must stop
            // instead of recreating the just-deleted row.
            return Ok(false);
        }

        *dispatch_record = dispatch_record.clone().into_preparing(summary);
        self.dispatch_repository()
            .save_dispatch(dispatch_record)
            .await?;
        tracing::info!(summary = %summary, "Updated task dispatch preparation status");

        Ok(true)
    }

    async fn append_follow_up_request_to_task(
        &self,
        task_id: &TaskId,
        follow_up_request: &str,
    ) -> Result<Task, TrackError> {
        let task = self.task_repository().get_task(task_id).await?;
        let timestamp_label = format_iso_8601_millis(now_utc());
        let next_description =
            append_follow_up_request(&task.description, &timestamp_label, follow_up_request);

        self.task_repository()
            .update_task(
                task_id,
                TaskUpdateInput {
                    description: Some(next_description),
                    priority: None,
                    status: None,
                },
            )
            .await
    }

    async fn load_dispatch_prerequisites(
        &self,
        task_id: &TaskId,
    ) -> Result<(Task, ProjectMetadata), TrackError> {
        let remote_agent = self.workspace.remote_agent();

        if remote_agent
            .shell_prelude
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "Remote runner setup is missing. Open the web UI and add the shell instructions that prepare PATH and toolchains for the remote runner.",
            ));
        }

        let task = self.task_repository().get_task(task_id).await?;
        let project = self
            .project_repository()
            .get_project_by_name(&task.project)
            .await?;
        validate_project_metadata_for_dispatch(&project.metadata)?;

        Ok((task, project.metadata))
    }
}

fn validate_project_metadata_for_dispatch(metadata: &ProjectMetadata) -> Result<(), TrackError> {
    if metadata.base_branch.trim().is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            "Project metadata must include repo URL, git URL, and base branch before dispatching a remote agent.",
        ));
    }

    parse_github_repository_name(&metadata.repo_url)?;
    Ok(())
}

fn dispatch_not_found(task_id: &TaskId, detail: &str) -> TrackError {
    TrackError::new(
        ErrorCode::DispatchNotFound,
        format!("Task {task_id} {detail}"),
    )
}

fn remote_cleanup_can_be_skipped(error: &TrackError) -> bool {
    matches!(error.code, ErrorCode::RemoteDispatchFailed)
}

pub(super) fn select_follow_up_base_dispatch(
    task_id: &TaskId,
    dispatch_history: &[TaskDispatchRecord],
) -> Result<TaskDispatchRecord, TrackError> {
    dispatch_history
        .iter()
        .find(|record| {
            !record.run.status.is_active()
                && record.run.branch_name.is_some()
                && record.run.worktree_path.is_some()
        })
        .cloned()
        .ok_or_else(|| {
            dispatch_not_found(
                task_id,
                "does not have a previous reusable remote dispatch to follow up on.",
            )
        })
}

pub(super) fn latest_pull_request_for_branch(
    dispatch_history: &[TaskDispatchRecord],
    branch_name: &DispatchBranch,
) -> Option<Url> {
    dispatch_history
        .iter()
        .find(|record| {
            record.run.branch_name.as_ref() == Some(branch_name)
                && record.pull_request_url.is_some()
        })
        .and_then(|record| record.pull_request_url.clone())
}

fn derive_remote_run_directory_for_record(
    record: &TaskDispatchRecord,
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Option<DispatchRunDirectory> {
    if let Some(worktree_path) = record.run.worktree_path.as_ref() {
        return Some(worktree_path.run_directory_for(&record.run.dispatch_id));
    }

    Some(DispatchRunDirectory::for_task(
        &remote_agent.workspace_root,
        &record.project,
        &record.run.dispatch_id,
    ))
}

pub(super) fn unique_worktree_paths(
    dispatch_history: &[TaskDispatchRecord],
) -> Vec<track_types::remote_layout::DispatchWorktreePath> {
    dispatch_history
        .iter()
        .filter_map(|record| record.run.worktree_path.as_ref())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn unique_run_directories(
    dispatch_history: &[TaskDispatchRecord],
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Vec<DispatchRunDirectory> {
    dispatch_history
        .iter()
        .filter_map(|record| derive_remote_run_directory_for_record(record, remote_agent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
