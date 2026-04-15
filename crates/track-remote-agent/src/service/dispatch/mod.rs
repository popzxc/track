use std::collections::BTreeSet;

use track_config::paths::collapse_home_path;
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
    DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentPreferredTool, Status, Task,
    TaskDispatchRecord, TaskUpdateInput,
};
use track_types::urls::Url;

use crate::constants::PREPARING_STALE_AFTER;
use crate::prompts::RemoteDispatchPrompt;
use crate::schemas::RemoteDispatchSchema;
use crate::types::{
    ClaudeStructuredOutputEnvelope, OpencodeStructuredOutput, RemoteArtifactCleanupCounts,
    RemoteTaskCleanupMode,
};
use crate::utils::parse_github_repository_name;
use crate::RemoteRunSnapshotView;
use crate::RemoteTaskArtifactCleanupMode;
use crate::RemoteWorkspace;

use super::log_remote_failure_output;
use super::remote_agent_services::{
    load_refresh_remote_workspace, RefreshRemoteWorkspace, RemoteAgentConfigProvider,
};

pub(crate) use self::guard::TaskDispatchStartGuard;
use self::record_ext::first_follow_up_line;

mod guard;
mod record_ext;

pub struct RemoteDispatchService<'a> {
    pub(super) config_service: &'a dyn RemoteAgentConfigProvider,
    pub(super) database: &'a DatabaseContext,
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
    // This file intentionally assembles the whole task-dispatch story in one
    // place: queueing, follow-up reuse, launch, refresh, and cleanup. The goal
    // is not to reduce responsibilities, but to make the current shape easy to
    // inspect while the service boundaries settle.
    #[tracing::instrument(skip(self), fields(task_id = %task_id, preferred_tool = ?preferred_tool))]
    pub async fn queue_dispatch(
        &self,
        task_id: &TaskId,
        preferred_tool: Option<RemoteAgentPreferredTool>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let (remote_agent, task, _project_metadata) =
            self.load_dispatch_prerequisites(task_id).await?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id).await?;
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
            dispatch_id = %dispatch_record.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.remote_host,
            branch_name = ?branch_name,
            worktree_path = ?worktree_path,
            preferred_tool = ?dispatch_record.preferred_tool,
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

        let (remote_agent, _task, _project_metadata) =
            self.load_dispatch_prerequisites(task_id).await?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id).await?;

        let dispatch_history = self
            .dispatch_repository()
            .dispatches_for_task(task_id)
            .await?;
        let previous_dispatch = select_follow_up_base_dispatch(task_id, &dispatch_history)?;
        let branch_name = previous_dispatch.branch_name.clone().ok_or_else(|| {
            dispatch_not_found(
                task_id,
                "does not have a reusable branch from the previous remote dispatch.",
            )
        })?;
        let worktree_path = previous_dispatch.worktree_path.clone().ok_or_else(|| {
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
                previous_dispatch.preferred_tool,
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
            dispatch_id = %dispatch_record.dispatch_id,
            base_dispatch_id = %previous_dispatch.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.remote_host,
            branch_name = ?branch_name,
            worktree_path = ?worktree_path,
            preferred_tool = ?dispatch_record.preferred_tool,
            follow_up_lines = trimmed_follow_up_request.lines().count(),
            "Queued remote task follow-up dispatch"
        );

        Ok(dispatch_record)
    }

    #[tracing::instrument(
        skip(self, dispatch_record),
        fields(
            task_id = %dispatch_record.task_id,
            dispatch_id = %dispatch_record.dispatch_id,
            project = %dispatch_record.project,
            remote_host = %dispatch_record.remote_host,
            preferred_tool = ?dispatch_record.preferred_tool
        )
    )]
    pub async fn launch_prepared_dispatch(
        &self,
        mut dispatch_record: TaskDispatchRecord,
    ) -> Result<TaskDispatchRecord, TrackError> {
        if let Some(existing_record) = self
            .dispatch_repository()
            .get_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)
            .await?
        {
            if !existing_record.status.is_active() {
                tracing::info!(
                    status = ?existing_record.status,
                    "Skipped launch because dispatch is no longer active"
                );
                return Ok(existing_record);
            }
        }

        let worktree_path = dispatch_record
            .worktree_path
            .clone()
            .expect("queued dispatches should always store a worktree path");
        let branch_name = dispatch_record
            .branch_name
            .clone()
            .expect("queued dispatches should always store a branch name");
        let launch_result = async {
            if !self
                .save_preparing_phase(&mut dispatch_record, "Checking remote agent prerequisites.")
                .await?
            {
                tracing::info!(
                    "Launch stopped before prerequisites because dispatch is no longer active"
                );
                return Ok::<(), TrackError>(());
            }
            let (remote_agent, task, project_metadata) = self
                .load_dispatch_prerequisites(&dispatch_record.task_id)
                .await?;
            tracing::info!(
                base_branch = %project_metadata.base_branch,
                workspace_root = %remote_agent.workspace_root,
                "Loaded task dispatch prerequisites"
            );
            let workspace = self.remote_workspace(remote_agent)?;
            if !self
                .save_preparing_phase(
                    &mut dispatch_record,
                    "Ensuring the remote checkout is up to date.",
                )
                .await?
            {
                tracing::info!(
                    "Launch stopped while refreshing checkout because dispatch is no longer active"
                );
                return Ok::<(), TrackError>(());
            }
            let checkout_path = workspace
                .projects()
                .ensure_task_checkout(&task.project, &project_metadata)?;
            tracing::info!(checkout_path = ?checkout_path, "Remote checkout is ready");

            if !self
                .save_preparing_phase(&mut dispatch_record, "Preparing the task worktree.")
                .await?
            {
                tracing::info!(
                    "Launch stopped while preparing worktree because dispatch is no longer active"
                );
                return Ok::<(), TrackError>(());
            }
            workspace.task_runs().prepare_worktree(
                &dispatch_record,
                &checkout_path,
                &project_metadata.base_branch,
                dispatch_record.follow_up_request.is_some(),
            )?;
            tracing::info!("Prepared remote task worktree");

            let prompt = RemoteDispatchPrompt::new(
                &task.project,
                &project_metadata,
                &branch_name,
                &worktree_path,
                &task.description,
                dispatch_record.pull_request_url.as_ref(),
                dispatch_record.follow_up_request.as_deref(),
            )
            .render();
            let schema = RemoteDispatchSchema.render();
            if !self
                .save_preparing_phase(
                    &mut dispatch_record,
                    "Uploading the agent prompt and schema.",
                )
                .await?
            {
                tracing::info!(
                    "Launch stopped while uploading run files because dispatch is no longer active"
                );
                return Ok::<(), TrackError>(());
            }
            workspace
                .task_runs()
                .upload_run_files(&dispatch_record, &prompt, &schema)?;
            tracing::info!("Uploaded remote task prompt and schema");

            // Cancellation can arrive while the API is still preparing the
            // remote checkout. We re-read the persisted record right before the
            // expensive remote-agent launch so a user-triggered cancel can stop
            // the flow before it starts spending more tokens.
            let dispatch_is_still_active = self
                .dispatch_repository()
                .get_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)
                .await?
                .map(|record| record.status.is_active())
                .unwrap_or(false);
            if !dispatch_is_still_active {
                tracing::info!("Launch stopped because dispatch was canceled during preparation");
                return Ok(());
            }

            if !self
                .save_preparing_phase(&mut dispatch_record, "Launching the remote agent.")
                .await?
            {
                tracing::info!(
                    "Launch stopped before remote agent start because dispatch is no longer active"
                );
                return Ok(());
            }
            workspace.task_runs().launch(&dispatch_record)?;
            tracing::info!("Started remote task agent process");

            Ok(())
        }
        .await;

        match launch_result {
            Ok(()) => {
                if let Some(existing_record) = self
                    .dispatch_repository()
                    .get_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)
                    .await?
                {
                    if !existing_record.status.is_active() {
                        let _ = self
                            .cancel_remote_dispatch_if_possible(&existing_record)
                            .await;
                        tracing::info!(
                            status = ?existing_record.status,
                            "Dispatch changed state before run promotion; returning persisted state"
                        );
                        return Ok(existing_record);
                    }
                }

                let dispatch_record = dispatch_record.into_running();
                self.dispatch_repository()
                    .save_dispatch(&dispatch_record)
                    .await?;
                tracing::info!("Marked dispatch as running");
                Ok(dispatch_record)
            }
            Err(error) => {
                tracing::error!(error = %error, "Remote task launch failed");
                let dispatch_record = dispatch_record.into_failed(error.to_string());
                self.dispatch_repository()
                    .save_dispatch(&dispatch_record)
                    .await?;
                Err(error)
            }
        }
    }

    // =============================================================================
    // Dispatch Cancellation And Discard
    // =============================================================================
    //
    // Cancellation and discard solve two different user intents:
    //
    // 1. "stop spending resources on this run" -> cancel the latest active run
    // 2. "forget the previous outcome and let me try again cleanly" -> discard
    //    the saved dispatch history for the task
    //
    // We keep them separate so the UI can expose both actions without
    // overloading one button with two meanings.
    #[tracing::instrument(skip(self), fields(task_id = %task_id))]
    pub async fn cancel_dispatch(
        &self,
        task_id: &TaskId,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let task_ids = [task_id.clone()];
        let mut latest_dispatch = self
            .latest_dispatches_for_tasks(&task_ids)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                dispatch_not_found(task_id, "does not have a remote dispatch to cancel.")
            })?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Task {task_id} does not have an active remote dispatch to cancel."),
            ));
        }

        self.cancel_remote_dispatch_if_possible(&latest_dispatch)
            .await?;

        latest_dispatch = latest_dispatch.into_canceled_from_ui();
        self.dispatch_repository()
            .save_dispatch(&latest_dispatch)
            .await?;

        tracing::info!(
            dispatch_id = %latest_dispatch.dispatch_id,
            remote_host = %latest_dispatch.remote_host,
            "Canceled remote task dispatch"
        );

        Ok(latest_dispatch)
    }

    #[tracing::instrument(skip(self), fields(task_id = %task_id))]
    pub async fn discard_dispatch_history(&self, task_id: &TaskId) -> Result<(), TrackError> {
        let latest_dispatch = self
            .dispatch_repository()
            .latest_dispatch_for_task(task_id)
            .await?
            .ok_or_else(|| {
                dispatch_not_found(task_id, "does not have a remote dispatch to discard.")
            })?;

        if latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Cancel the active remote dispatch before discarding its history.",
            ));
        }

        // Discard intentionally clears the entire visible dispatch history for
        // the task so the card goes back to an undecorated state. That matches
        // the UI intent of "let me try this task again from a clean slate".
        // TODO: This currently leaves remote worktrees and dispatch directories
        // alone on purpose. Product policy only asked for remote cleanup on
        // task close/delete, not on discard.
        // TODO: If users later want audit history, replace this hard delete
        // with an explicit archived-history concept instead of reviving older
        // dispatch records automatically.
        self.dispatch_repository()
            .delete_dispatch_history_for_task(task_id)
            .await?;

        tracing::info!(
            latest_dispatch_id = %latest_dispatch.dispatch_id,
            "Discarded task dispatch history"
        );

        Ok(())
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
    // Global Dispatch History
    // =============================================================================
    //
    // The frontend's Runs page needs the same "what is the remote machine
    // saying right now?" view as the task-level dispatch badges. We therefore
    // route the global history listing through the same refresh path instead of
    // reading raw JSON records and leaving active runs stale until some other
    // endpoint happens to reconcile them.
    pub async fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self.dispatch_repository().list_dispatches(limit).await?;
        self.refresh_active_dispatch_records(records).await
    }

    // The task drawer needs authoritative history for the selected task even
    // when the global Runs page is intentionally truncated for UI cost. We
    // therefore expose a task-scoped history path that keeps older tasks from
    // losing their latest status or drawer history just because newer runs
    // pushed them past the global limit.
    pub async fn dispatch_history_for_task(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut records = self
            .dispatch_repository()
            .dispatches_for_task(task_id)
            .await?;

        // At most one dispatch should be active per task. If the newest record
        // is still active, route it through the same remote reconciliation path
        // as the queue badges so the drawer sees current state instead of raw
        // persisted JSON.
        if records
            .first()
            .is_some_and(|record| record.status.is_active())
        {
            if let Some(refreshed_latest) = self
                .latest_dispatches_for_tasks(std::slice::from_ref(task_id))
                .await?
                .into_iter()
                .next()
            {
                if let Some(first_record) = records.first_mut() {
                    *first_record = refreshed_latest;
                }
            }
        }

        Ok(records)
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

                // The tracker should stay usable even if the remote machine,
                // SSH key, or remote config disappears. Closing the task is a
                // local filesystem mutation first; remote cleanup is only a
                // best-effort follow-up.
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
                    Err(error) => {
                        tracing::warn!(error = %error, "Skipping remote cleanup while closing task");
                        self.finalize_active_dispatches_locally(
                            &dispatch_history,
                            DispatchStatus::Canceled,
                            "Canceled because the task was closed locally. Remote cleanup was skipped.",
                            Some(error.message()),
                        )
                        .await?;
                    }
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
                // Delete is the strongest local intent: once the user removes a
                // task, stale remote artifacts must not veto that choice.
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

    async fn refresh_active_dispatch_records(
        &self,
        records: Vec<TaskDispatchRecord>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let workspace = match load_refresh_remote_workspace(self.config_service, self.database)
            .await?
        {
            RefreshRemoteWorkspace::Available(workspace) => workspace,
            RefreshRemoteWorkspace::UnavailableLocally { error_message } => {
                return self
                    .release_active_dispatches_after_reconciliation_loss(
                        records,
                        "Remote reconciliation is unavailable locally, so active runs were released.",
                        &error_message,
                    )
                    .await;
            }
        };
        let snapshots_by_dispatch_id = match workspace
            .task_runs()
            .load_snapshots_for_active_records(&records)
        {
            Ok(snapshots) => snapshots,
            Err(error) => {
                let error_message = error.to_string();
                return self
                    .release_active_dispatches_after_reconciliation_loss(
                        records,
                        "Remote reconciliation could not reach the remote machine, so active runs were released locally.",
                        &error_message,
                    )
                    .await;
            }
        };
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(record.dispatch_id.as_str()) else {
                if let Some(updated) = record
                    .clone()
                    .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
                {
                    self.dispatch_repository().save_dispatch(&updated).await?;
                    tracing::info!(
                        dispatch_id = %updated.dispatch_id,
                        status = ?updated.status,
                        "Marked stale preparing dispatch as abandoned during refresh"
                    );
                    refreshed_records.push(updated);
                } else {
                    let updated = self.finalize_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        "Remote reconciliation could not find this run anymore, so it was released locally.",
                        Some("Remote dispatch snapshot is missing."),
                    )
                    .await?;
                    tracing::warn!(
                        dispatch_id = %updated.dispatch_id,
                        status = ?updated.status,
                        "Released active dispatch because its remote snapshot is missing"
                    );
                    refreshed_records.push(updated);
                }
                continue;
            };

            match refresh_dispatch_record_from_snapshot(record.clone(), snapshot) {
                Ok(updated) => {
                    if updated != record {
                        self.dispatch_repository().save_dispatch(&updated).await?;
                        tracing::info!(
                            dispatch_id = %updated.dispatch_id,
                            previous_status = ?record.status,
                            status = ?updated.status,
                            "Refreshed task dispatch state from remote snapshot"
                        );
                    }
                    refreshed_records.push(updated);
                }
                Err(error) => {
                    tracing::error!(
                        dispatch_id = %record.dispatch_id,
                        error = %error,
                        observed_status = ?snapshot.status,
                        "Failed to interpret remote task snapshot"
                    );
                    log_remote_failure_output(
                        snapshot.result.as_deref(),
                        snapshot.stderr.as_deref(),
                    );
                    if snapshot.is_finished() {
                        let refreshed_at = now_utc();
                        let finished_at = snapshot.finished_at_or(refreshed_at);
                        let updated = record.clone().mark_failed_from_remote_refresh(
                            refreshed_at,
                            finished_at,
                            error.to_string(),
                        );
                        self.dispatch_repository().save_dispatch(&updated).await?;
                        tracing::warn!(
                            dispatch_id = %updated.dispatch_id,
                            status = ?updated.status,
                            "Marked dispatch as failed after remote snapshot parse error"
                        );
                        refreshed_records.push(updated);
                    } else {
                        let error_message = error.to_string();
                        let updated = self.finalize_dispatch_locally(
                            &record,
                            DispatchStatus::Blocked,
                            "Remote reconciliation could not confirm this run, so it was released locally.",
                            Some(&error_message),
                        )
                        .await?;
                        tracing::warn!(
                            dispatch_id = %updated.dispatch_id,
                            status = ?updated.status,
                            "Released active dispatch after remote reconciliation error"
                        );
                        refreshed_records.push(updated);
                    }
                }
            }
        }

        Ok(refreshed_records)
    }

    async fn release_active_dispatches_after_reconciliation_loss(
        &self,
        records: Vec<TaskDispatchRecord>,
        summary: &str,
        error_message: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if record.status.is_active() {
                refreshed_records.push(
                    self.finalize_dispatch_locally(
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

    pub(super) async fn cleanup_task_remote_artifacts(
        &self,
        task_id: &TaskId,
        dispatch_history: &[TaskDispatchRecord],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        if dispatch_history.is_empty() {
            return Ok(RemoteArtifactCleanupCounts::default());
        }

        let remote_agent = self.load_remote_agent(task_id).await?;
        let workspace = self.remote_workspace(remote_agent)?;
        let checkout_path = workspace
            .projects()
            .resolve_checkout_path_for_project(&dispatch_history[0].project);
        let summary = workspace.task_runs().cleanup(
            &checkout_path,
            dispatch_history,
            match cleanup_mode {
                RemoteTaskCleanupMode::CloseTask => RemoteTaskArtifactCleanupMode::CloseTask,
                RemoteTaskCleanupMode::DeleteTask => RemoteTaskArtifactCleanupMode::DeleteTask,
            },
        )?;

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
            if !dispatch_record.status.is_active() {
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
                dispatch_id = %updated_record.dispatch_id,
                status = ?updated_record.status,
                summary = %summary,
                error_message = error_message.unwrap_or(""),
                "Locally finalized task dispatch after remote disruption"
            );
        } else {
            tracing::info!(
                dispatch_id = %updated_record.dispatch_id,
                status = ?updated_record.status,
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
            .filter(|record| record.status.is_active())
        {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Task {task_id} already has an active remote dispatch ({})",
                    existing_dispatch.dispatch_id
                ),
            ));
        }

        Ok(())
    }

    async fn cancel_remote_dispatch_if_possible(
        &self,
        dispatch_record: &TaskDispatchRecord,
    ) -> Result<(), TrackError> {
        let remote_agent = self.load_remote_agent(&dispatch_record.task_id).await?;

        let Some(worktree_path) = dispatch_record.worktree_path.as_ref() else {
            return Ok(());
        };
        let workspace = self.remote_workspace(remote_agent)?;
        let _ = worktree_path;
        workspace.task_runs().cancel(dispatch_record).map(|_| ())?;
        tracing::info!(
            dispatch_id = %dispatch_record.dispatch_id,
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
            .get_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)
            .await?
        {
            if !saved_record.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
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
    ) -> Result<(RemoteAgentRuntimeConfig, Task, ProjectMetadata), TrackError> {
        let remote_agent = self.load_remote_agent(task_id).await?;

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

        Ok((remote_agent, task, project.metadata))
    }

    async fn load_remote_agent(
        &self,
        task_id: &TaskId,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()
            .await?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    format!("Task {task_id}: Remote agent configuration is missing."),
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Task {task_id}: Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before cleaning task.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        Ok(remote_agent)
    }

    fn remote_workspace(
        &self,
        remote_agent: RemoteAgentRuntimeConfig,
    ) -> Result<RemoteWorkspace, TrackError> {
        RemoteWorkspace::new(remote_agent, self.database.clone())
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

pub(super) fn select_follow_up_base_dispatch(
    task_id: &TaskId,
    dispatch_history: &[TaskDispatchRecord],
) -> Result<TaskDispatchRecord, TrackError> {
    dispatch_history
        .iter()
        .find(|record| {
            !record.status.is_active()
                && record.branch_name.is_some()
                && record.worktree_path.is_some()
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
            record.branch_name.as_ref() == Some(branch_name) && record.pull_request_url.is_some()
        })
        .and_then(|record| record.pull_request_url.clone())
}

fn derive_remote_run_directory_for_record(
    record: &TaskDispatchRecord,
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Option<DispatchRunDirectory> {
    if let Some(worktree_path) = record.worktree_path.as_ref() {
        return Some(worktree_path.run_directory_for(&record.dispatch_id));
    }

    Some(DispatchRunDirectory::for_task(
        &remote_agent.workspace_root,
        &record.project,
        &record.dispatch_id,
    ))
}

pub(super) fn refresh_dispatch_record_from_snapshot(
    record: TaskDispatchRecord,
    snapshot: &RemoteRunSnapshotView,
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
        let outcome = match record.preferred_tool {
            RemoteAgentPreferredTool::Opencode => OpencodeStructuredOutput::<
                RemoteAgentDispatchOutcome,
            >::parse_result(
                remote_result, "Remote agent result"
            )?,
            RemoteAgentPreferredTool::Claude => {
                ClaudeStructuredOutputEnvelope::<RemoteAgentDispatchOutcome>::parse_result(
                    remote_result,
                    record.preferred_tool,
                    "Remote agent result",
                )?
            }
            RemoteAgentPreferredTool::Codex => serde_json::from_str::<RemoteAgentDispatchOutcome>(
                remote_result,
            )
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote agent result is not valid JSON: {error}"),
                )
            })?,
        };
        return Ok(record.apply_remote_dispatch_outcome(outcome, refreshed_at, finished_at));
    }

    Ok(record.mark_failed_from_remote_refresh(
        refreshed_at,
        finished_at,
        snapshot.failure_message("Remote agent run failed before returning a structured result."),
    ))
}

pub(super) fn unique_worktree_paths(
    dispatch_history: &[TaskDispatchRecord],
) -> Vec<track_types::remote_layout::DispatchWorktreePath> {
    dispatch_history
        .iter()
        .filter_map(|record| record.worktree_path.as_ref())
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
