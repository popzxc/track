use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Condvar, Mutex, OnceLock};

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::task_repository::FileTaskRepository;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::task_description::append_follow_up_request;
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{
    DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentPreferredTool, Status, Task,
    TaskDispatchRecord, TaskUpdateInput,
};

use crate::constants::{PREPARING_STALE_AFTER, REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME};
use crate::prompts::RemoteDispatchPrompt;
use crate::schemas::RemoteDispatchSchema;
use crate::ssh::SshClient;
use crate::types::{
    ClaudeStructuredOutputEnvelope, RemoteArtifactCleanupCounts, RemoteDispatchSnapshot,
    RemoteTaskCleanupMode,
};
use crate::utils::parse_github_repository_name;

use super::remote_agent_services::{
    load_refresh_ssh_client, RefreshRemoteClient, RemoteAgentConfigProvider, RemoteRunOps,
    RemoteWorkspaceOps,
};

pub struct RemoteDispatchService<'a> {
    pub(in crate::service) config_service: &'a dyn RemoteAgentConfigProvider,
    pub(in crate::service) dispatch_repository: &'a DispatchRepository,
    pub(in crate::service) project_repository: &'a ProjectRepository,
    pub(in crate::service) task_repository: &'a FileTaskRepository,
}

impl<'a> RemoteDispatchService<'a> {
    // =============================================================================
    // Remote Dispatch Entry Points
    // =============================================================================
    //
    // This file intentionally assembles the whole task-dispatch story in one
    // place: queueing, follow-up reuse, launch, refresh, and cleanup. The goal
    // is not to reduce responsibilities, but to make the current shape easy to
    // inspect while the service boundaries settle.
    pub fn queue_dispatch(
        &self,
        task_id: &str,
        preferred_tool: Option<RemoteAgentPreferredTool>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let (remote_agent, task, _project_metadata) = self.load_dispatch_prerequisites(task_id)?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id)?;
        let preferred_tool = preferred_tool.unwrap_or(remote_agent.preferred_tool);

        let dispatch_record = self
            .dispatch_repository
            .create_dispatch(&task, &remote_agent.host, preferred_tool)?
            .populated(&remote_agent, &task);
        self.dispatch_repository.save_dispatch(&dispatch_record)?;

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
    pub fn queue_follow_up_dispatch(
        &self,
        task_id: &str,
        follow_up_request: &str,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let trimmed_follow_up_request = follow_up_request.trim();
        if trimmed_follow_up_request.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a follow-up request for the remote agent.",
            ));
        }

        let (remote_agent, _task, _project_metadata) = self.load_dispatch_prerequisites(task_id)?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id)?;

        let dispatch_history = self.dispatch_repository.dispatches_for_task(task_id)?;
        let previous_dispatch = select_follow_up_base_dispatch(&dispatch_history).ok_or_else(
            task_dispatch_not_found(
                task_id,
                "does not have a previous reusable remote dispatch to follow up on.",
            ),
        )?;
        let branch_name =
            previous_dispatch
                .branch_name
                .clone()
                .ok_or_else(task_dispatch_not_found(
                    task_id,
                    "does not have a reusable branch from the previous remote dispatch.",
                ))?;
        let worktree_path =
            previous_dispatch
                .worktree_path
                .clone()
                .ok_or_else(task_dispatch_not_found(
                    task_id,
                    "does not have a reusable worktree from the previous remote dispatch.",
                ))?;

        let updated_task =
            self.append_follow_up_request_to_task(task_id, trimmed_follow_up_request)?;
        let pull_request_url = latest_pull_request_for_branch(&dispatch_history, &branch_name)
            .or(previous_dispatch.pull_request_url.clone());
        let dispatch_record = self
            .dispatch_repository
            .create_dispatch(
                &updated_task,
                &remote_agent.host,
                previous_dispatch.preferred_tool,
            )?
            .populated_follow_up(
                branch_name,
                worktree_path,
                pull_request_url,
                trimmed_follow_up_request,
                &previous_dispatch,
            );
        self.dispatch_repository.save_dispatch(&dispatch_record)?;

        Ok(dispatch_record)
    }

    pub fn launch_prepared_dispatch(
        &self,
        mut dispatch_record: TaskDispatchRecord,
    ) -> Result<TaskDispatchRecord, TrackError> {
        if let Some(existing_record) =
            self.load_saved_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)?
        {
            if !existing_record.status.is_active() {
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
        let remote_run_directory =
            derive_remote_run_directory(&worktree_path, &dispatch_record.dispatch_id)?;

        let launch_result = (|| -> Result<(), TrackError> {
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Checking remote agent prerequisites.",
            )? {
                return Ok(());
            }
            let (remote_agent, task, project_metadata) =
                self.load_dispatch_prerequisites(&dispatch_record.task_id)?;
            let ssh_client = SshClient::new(&remote_agent)?;
            let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
            let runner = RemoteRunOps::new(&ssh_client);
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Ensuring the remote checkout is up to date.",
            )? {
                return Ok(());
            }
            let checkout_path = workspace.ensure_task_checkout(&task.project, &project_metadata)?;

            if !self.save_preparing_phase(&mut dispatch_record, "Preparing the task worktree.")? {
                return Ok(());
            }
            workspace.prepare_task_worktree(
                &checkout_path,
                &project_metadata.base_branch,
                &branch_name,
                &worktree_path,
                dispatch_record.follow_up_request.is_some(),
            )?;

            let prompt = RemoteDispatchPrompt::new(
                &task.project,
                &project_metadata,
                &branch_name,
                &worktree_path,
                &task.description,
                dispatch_record.pull_request_url.as_deref(),
                dispatch_record.follow_up_request.as_deref(),
            )
            .render();
            let schema = RemoteDispatchSchema.render();
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Uploading the agent prompt and schema.",
            )? {
                return Ok(());
            }
            runner.upload_prompt_and_schema(
                &format!("{remote_run_directory}/{REMOTE_PROMPT_FILE_NAME}"),
                &prompt,
                &format!("{remote_run_directory}/{REMOTE_SCHEMA_FILE_NAME}"),
                &schema,
            )?;

            // Cancellation can arrive while the API is still preparing the
            // remote checkout. We re-read the persisted record right before the
            // expensive remote-agent launch so a user-triggered cancel can stop
            // the flow before it starts spending more tokens.
            if !self
                .dispatch_is_still_active(&dispatch_record.task_id, &dispatch_record.dispatch_id)?
            {
                return Ok(());
            }

            if !self.save_preparing_phase(&mut dispatch_record, "Launching the remote agent.")? {
                return Ok(());
            }
            runner.launch(
                &remote_run_directory,
                &worktree_path,
                dispatch_record.preferred_tool,
            )?;

            Ok(())
        })();

        match launch_result {
            Ok(()) => {
                if let Some(existing_record) = self
                    .load_saved_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)?
                {
                    if !existing_record.status.is_active() {
                        let _ = self.cancel_remote_dispatch_if_possible(&existing_record);
                        return Ok(existing_record);
                    }
                }

                let dispatch_record = dispatch_record.into_running();
                self.dispatch_repository.save_dispatch(&dispatch_record)?;
                Ok(dispatch_record)
            }
            Err(error) => {
                let dispatch_record = dispatch_record.into_failed(error.to_string());
                self.dispatch_repository.save_dispatch(&dispatch_record)?;
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
    pub fn cancel_dispatch(&self, task_id: &str) -> Result<TaskDispatchRecord, TrackError> {
        let mut latest_dispatch = self
            .latest_dispatches_for_tasks(&[task_id.to_owned()])?
            .into_iter()
            .next()
            .ok_or_else(task_dispatch_not_found(
                task_id,
                "does not have a remote dispatch to cancel.",
            ))?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Task {task_id} does not have an active remote dispatch to cancel."),
            ));
        }

        self.cancel_remote_dispatch_if_possible(&latest_dispatch)?;

        latest_dispatch = latest_dispatch.into_canceled_from_ui();
        self.dispatch_repository.save_dispatch(&latest_dispatch)?;

        Ok(latest_dispatch)
    }

    pub fn discard_dispatch_history(&self, task_id: &str) -> Result<(), TrackError> {
        let latest_dispatch = self
            .dispatch_repository
            .latest_dispatch_for_task(task_id)?
            .ok_or_else(task_dispatch_not_found(
                task_id,
                "does not have a remote dispatch to discard.",
            ))?;

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
        self.dispatch_repository
            .delete_dispatch_history_for_task(task_id)
    }

    pub fn latest_dispatches_for_tasks(
        &self,
        task_ids: &[String],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self
            .dispatch_repository
            .latest_dispatches_for_tasks(task_ids)?;
        self.refresh_active_dispatch_records(records)
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
    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let records = self.dispatch_repository.list_dispatches(limit)?;
        self.refresh_active_dispatch_records(records)
    }

    // The task drawer needs authoritative history for the selected task even
    // when the global Runs page is intentionally truncated for UI cost. We
    // therefore expose a task-scoped history path that keeps older tasks from
    // losing their latest status or drawer history just because newer runs
    // pushed them past the global limit.
    pub fn dispatch_history_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut records = self.dispatch_repository.dispatches_for_task(task_id)?;

        // At most one dispatch should be active per task. If the newest record
        // is still active, route it through the same remote reconciliation path
        // as the queue badges so the drawer sees current state instead of raw
        // persisted JSON.
        if records
            .first()
            .is_some_and(|record| record.status.is_active())
        {
            if let Some(refreshed_latest) = self
                .latest_dispatches_for_tasks(&[task_id.to_owned()])?
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

    fn refresh_active_dispatch_records(
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

    pub(in crate::service) fn cleanup_task_remote_artifacts(
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
        let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
        let checkout_path = workspace.resolve_checkout_path(&dispatch_history[0].project)?;
        let worktree_paths = unique_worktree_paths(dispatch_history);
        let run_directories = unique_run_directories(dispatch_history, &remote_agent);

        workspace.cleanup_task_artifacts(
            &checkout_path,
            &worktree_paths,
            &run_directories,
            cleanup_mode,
        )
    }

    pub(in crate::service) fn finalize_active_dispatches_locally(
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

    fn finalize_dispatch_locally(
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
        self.dispatch_repository.save_dispatch(&updated_record)?;

        Ok(updated_record)
    }

    fn ensure_no_blocking_active_dispatch(&self, task_id: &str) -> Result<(), TrackError> {
        if let Some(existing_dispatch) = self
            .latest_dispatches_for_tasks(&[task_id.to_owned()])?
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

    fn dispatch_is_still_active(
        &self,
        task_id: &str,
        dispatch_id: &str,
    ) -> Result<bool, TrackError> {
        Ok(self
            .load_saved_dispatch(task_id, dispatch_id)?
            .map(|record| record.status.is_active())
            .unwrap_or(false))
    }

    fn load_saved_dispatch(
        &self,
        task_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        self.dispatch_repository.get_dispatch(task_id, dispatch_id)
    }

    fn cancel_remote_dispatch_if_possible(
        &self,
        dispatch_record: &TaskDispatchRecord,
    ) -> Result<(), TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        let Some(worktree_path) = dispatch_record.worktree_path.as_deref() else {
            return Ok(());
        };
        let remote_run_directory =
            derive_remote_run_directory(worktree_path, &dispatch_record.dispatch_id)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        RemoteRunOps::new(&ssh_client).cancel(&remote_run_directory)
    }

    fn save_preparing_phase(
        &self,
        dispatch_record: &mut TaskDispatchRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) =
            self.load_saved_dispatch(&dispatch_record.task_id, &dispatch_record.dispatch_id)?
        {
            if !saved_record.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
        }

        *dispatch_record = dispatch_record.clone().into_preparing(summary);
        self.dispatch_repository.save_dispatch(dispatch_record)?;

        Ok(true)
    }

    fn append_follow_up_request_to_task(
        &self,
        task_id: &str,
        follow_up_request: &str,
    ) -> Result<Task, TrackError> {
        let task = self.task_repository.get_task(task_id)?;
        let timestamp_label = format_iso_8601_millis(now_utc());
        let next_description =
            append_follow_up_request(&task.description, &timestamp_label, follow_up_request);

        self.task_repository.update_task(
            task_id,
            TaskUpdateInput {
                description: Some(next_description),
                priority: None,
                status: None,
            },
        )
    }

    fn load_dispatch_prerequisites(
        &self,
        task_id: &str,
    ) -> Result<(RemoteAgentRuntimeConfig, Task, ProjectMetadata), TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

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

        let task = self.task_repository.get_task(task_id)?;
        let project = self.project_repository.get_project_by_name(&task.project)?;
        validate_project_metadata_for_dispatch(&project.metadata)?;

        Ok((remote_agent, task, project.metadata))
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
}

fn validate_project_metadata_for_dispatch(metadata: &ProjectMetadata) -> Result<(), TrackError> {
    if metadata.repo_url.trim().is_empty()
        || metadata.git_url.trim().is_empty()
        || metadata.base_branch.trim().is_empty()
    {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            "Project metadata must include repo URL, git URL, and base branch before dispatching a remote agent.",
        ));
    }

    parse_github_repository_name(&metadata.repo_url)?;
    Ok(())
}

// =============================================================================
// Task Dispatch Record Transitions
// =============================================================================
//
// `TaskDispatchRecord` lives in shared types, but this file owns the dispatch-
// specific meaning of "queued", "follow-up", "preparing", and similar states.
// A local extension trait keeps those transitions readable without pretending
// the shared type should expose these domain rules everywhere.
trait TaskDispatchRecordExt {
    fn populated(self, remote_agent: &RemoteAgentRuntimeConfig, task: &Task) -> Self;
    fn populated_follow_up(
        self,
        branch_name: String,
        worktree_path: String,
        pull_request_url: Option<String>,
        follow_up_request: &str,
        previous_dispatch: &TaskDispatchRecord,
    ) -> Self;
    fn into_preparing(self, summary: &str) -> Self;
    fn into_running(self) -> Self;
    fn into_failed(self, error_message: String) -> Self;
    fn into_canceled_from_ui(self) -> Self;
    fn into_locally_finalized(
        self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self;
}

impl TaskDispatchRecordExt for TaskDispatchRecord {
    fn populated(mut self, remote_agent: &RemoteAgentRuntimeConfig, task: &Task) -> Self {
        self.branch_name = Some(format!("track/{}", self.dispatch_id));
        self.worktree_path = Some(format!(
            "{}/{}/worktrees/{}",
            remote_agent.workspace_root.trim_end_matches('/'),
            task.project,
            self.dispatch_id
        ));
        self.updated_at = now_utc();
        self
    }

    fn populated_follow_up(
        mut self,
        branch_name: String,
        worktree_path: String,
        pull_request_url: Option<String>,
        follow_up_request: &str,
        previous_dispatch: &TaskDispatchRecord,
    ) -> Self {
        self.branch_name = Some(branch_name);
        self.worktree_path = Some(worktree_path);
        self.pull_request_url = pull_request_url;
        self.follow_up_request = Some(follow_up_request.to_owned());
        self.review_request_head_oid = previous_dispatch.review_request_head_oid.clone();
        self.review_request_user = previous_dispatch.review_request_user.clone();
        self.summary = Some(format!(
            "Follow-up request: {}",
            first_follow_up_line(follow_up_request)
        ));
        self.updated_at = now_utc();
        self
    }

    fn into_preparing(mut self, summary: &str) -> Self {
        self.status = DispatchStatus::Preparing;
        self.summary = Some(summary.to_owned());
        self.updated_at = now_utc();
        self.finished_at = None;
        self.error_message = None;
        self
    }

    fn into_running(mut self) -> Self {
        self.status = DispatchStatus::Running;
        self.updated_at = now_utc();
        self.finished_at = None;
        self.summary = Some("The remote agent is working in the prepared environment.".to_owned());
        self.error_message = None;
        self
    }

    fn into_failed(mut self, error_message: String) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = now_utc();
        self.finished_at = Some(self.updated_at);
        self.error_message = Some(error_message);
        self
    }

    fn into_canceled_from_ui(self) -> Self {
        self.into_locally_finalized(DispatchStatus::Canceled, "Canceled from the web UI.", None)
    }

    fn into_locally_finalized(
        mut self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self {
        let finished_at = now_utc();
        self.status = status;
        self.updated_at = finished_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(summary.to_owned());
        self.notes = None;
        self.error_message = error_message.map(ToOwned::to_owned);
        self
    }
}

fn task_dispatch_not_found<'a>(
    task_id: &'a str,
    detail: &'a str,
) -> impl FnOnce() -> TrackError + 'a {
    move || {
        TrackError::new(
            ErrorCode::DispatchNotFound,
            format!("Task {task_id} {detail}"),
        )
    }
}

fn first_follow_up_line(follow_up_request: &str) -> String {
    follow_up_request
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Continue the previous remote task.")
        .to_owned()
}

pub(in crate::service) fn select_follow_up_base_dispatch(
    dispatch_history: &[TaskDispatchRecord],
) -> Option<TaskDispatchRecord> {
    dispatch_history
        .iter()
        .find(|record| {
            !record.status.is_active()
                && record.branch_name.is_some()
                && record.worktree_path.is_some()
        })
        .cloned()
}

pub(in crate::service) fn latest_pull_request_for_branch(
    dispatch_history: &[TaskDispatchRecord],
    branch_name: &str,
) -> Option<String> {
    dispatch_history
        .iter()
        .find(|record| {
            record.branch_name.as_deref() == Some(branch_name)
                && record
                    .pull_request_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_some()
        })
        .and_then(|record| record.pull_request_url.clone())
}

fn load_dispatch_snapshots_for_records(
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

    let snapshots = RemoteRunOps::new(ssh_client).read_snapshots(&run_directories)?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}

fn derive_remote_run_directory(
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

fn derive_remote_run_directory_for_record(
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

pub(in crate::service) fn refresh_dispatch_record_from_snapshot(
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

pub(in crate::service) fn unique_worktree_paths(
    dispatch_history: &[TaskDispatchRecord],
) -> Vec<String> {
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

pub(in crate::service) fn unique_run_directories(
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

#[derive(Debug, Default)]
struct TaskDispatchStartGate {
    active_task_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
pub(crate) struct TaskDispatchStartGuard {
    task_id: String,
}

impl TaskDispatchStartGuard {
    pub(crate) fn acquire(task_id: &str) -> Self {
        let gate = task_dispatch_start_gate();
        let mut active_task_ids = gate
            .active_task_ids
            .lock()
            .expect("dispatch start gate should not be poisoned");

        while active_task_ids.contains(task_id) {
            active_task_ids = gate
                .wake_waiters
                .wait(active_task_ids)
                .expect("dispatch start gate should not be poisoned");
        }

        active_task_ids.insert(task_id.to_owned());

        Self {
            task_id: task_id.to_owned(),
        }
    }
}

impl Drop for TaskDispatchStartGuard {
    fn drop(&mut self) {
        let gate = task_dispatch_start_gate();
        let mut active_task_ids = gate
            .active_task_ids
            .lock()
            .expect("dispatch start gate should not be poisoned");
        active_task_ids.remove(&self.task_id);
        gate.wake_waiters.notify_all();
    }
}

fn task_dispatch_start_gate() -> &'static TaskDispatchStartGate {
    static GATE: OnceLock<TaskDispatchStartGate> = OnceLock::new();

    // Dispatch start requests are handled by one long-lived API process in the
    // deployed shape, so a process-local gate is enough to close the race
    // between "no active dispatch exists" and "persist a new preparing record".
    // This keeps the fix lightweight and avoids inventing filesystem locks for
    // a code path that only needs in-process serialization.
    GATE.get_or_init(TaskDispatchStartGate::default)
}
