use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::task_description::append_follow_up_request;
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{
    DispatchStatus, RemoteAgentPreferredTool, Task, TaskDispatchRecord, TaskUpdateInput,
};

use crate::constants::{REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME};
use crate::prompts::RemoteDispatchPrompt;
use crate::remote_actions::{
    CancelRemoteDispatchAction, CreateWorktreeAction, EnsureCheckoutAction,
    EnsureFollowUpWorktreeAction, FetchGithubLoginAction, LaunchRemoteDispatchAction,
    LoadRemoteRegistryAction, UploadRemoteFileAction, WriteRemoteRegistryAction,
};
use crate::schemas::RemoteDispatchSchema;
use crate::ssh::SshClient;
use crate::types::RemoteProjectRegistryEntry;
use crate::utils::parse_github_repository_name;

use super::follow_up::{
    first_follow_up_line, latest_pull_request_for_branch, select_follow_up_base_dispatch,
};
use super::refresh::derive_remote_run_directory;
use super::start_gate::TaskDispatchStartGuard;
use super::RemoteDispatchService;

impl<'a> RemoteDispatchService<'a> {
    // =============================================================================
    // Remote Dispatch Entry Points
    // =============================================================================
    //
    // Dispatch orchestration is split into two phases so the API can persist a
    // visible "preparing environment" state immediately. The browser receives
    // that state right away, while the slower SSH/bootstrap work continues in
    // the background and later transitions the record into `running` or a
    // terminal outcome.
    pub fn queue_dispatch(
        &self,
        task_id: &str,
        preferred_tool: Option<RemoteAgentPreferredTool>,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let (remote_agent, task, _project_metadata) = self.load_dispatch_prerequisites(task_id)?;
        let _dispatch_start_guard = TaskDispatchStartGuard::acquire(task_id);
        self.ensure_no_blocking_active_dispatch(task_id)?;
        let preferred_tool = preferred_tool.unwrap_or(remote_agent.preferred_tool);

        let mut dispatch_record =
            self.dispatch_repository
                .create_dispatch(&task, &remote_agent.host, preferred_tool)?;
        dispatch_record.branch_name = Some(format!("track/{}", dispatch_record.dispatch_id));
        dispatch_record.worktree_path = Some(format!(
            "{}/{}/worktrees/{}",
            remote_agent.workspace_root.trim_end_matches('/'),
            task.project,
            dispatch_record.dispatch_id
        ));
        dispatch_record.updated_at = now_utc();
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
        let previous_dispatch = select_follow_up_base_dispatch(&dispatch_history)
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::DispatchNotFound,
                    format!(
                        "Task {task_id} does not have a previous reusable remote dispatch to follow up on."
                    ),
                )
            })?;
        let branch_name = previous_dispatch.branch_name.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::DispatchNotFound,
                format!(
                    "Task {task_id} does not have a reusable branch from the previous remote dispatch."
                ),
            )
        })?;
        let worktree_path = previous_dispatch.worktree_path.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::DispatchNotFound,
                format!(
                    "Task {task_id} does not have a reusable worktree from the previous remote dispatch."
                ),
            )
        })?;

        let updated_task =
            self.append_follow_up_request_to_task(task_id, trimmed_follow_up_request)?;
        let mut dispatch_record = self.dispatch_repository.create_dispatch(
            &updated_task,
            &remote_agent.host,
            previous_dispatch.preferred_tool,
        )?;
        dispatch_record.branch_name = Some(branch_name);
        dispatch_record.worktree_path = Some(worktree_path);
        dispatch_record.pull_request_url = latest_pull_request_for_branch(
            &dispatch_history,
            dispatch_record
                .branch_name
                .as_deref()
                .expect("follow-up dispatches should always have a branch name"),
        )
        .or(previous_dispatch.pull_request_url.clone());
        dispatch_record.follow_up_request = Some(trimmed_follow_up_request.to_owned());
        dispatch_record.review_request_head_oid = previous_dispatch.review_request_head_oid.clone();
        dispatch_record.review_request_user = previous_dispatch.review_request_user.clone();
        dispatch_record.summary = Some(format!(
            "Follow-up request: {}",
            first_follow_up_line(trimmed_follow_up_request)
        ));
        dispatch_record.updated_at = now_utc();
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
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Loading the remote project registry.",
            )? {
                return Ok(());
            }
            let remote_registry =
                LoadRemoteRegistryAction::new(&ssh_client, &remote_agent.projects_registry_path)
                    .execute()?;
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Checking GitHub authentication on the remote machine.",
            )? {
                return Ok(());
            }
            let github_login = FetchGithubLoginAction::new(&ssh_client).execute()?;
            let repository_name = parse_github_repository_name(&project_metadata.repo_url)?;
            let checkout_path = remote_registry
                .projects
                .get(&task.project)
                .map(|entry| entry.checkout_path.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{}/{}/{}",
                        remote_agent.workspace_root.trim_end_matches('/'),
                        task.project,
                        task.project
                    )
                });

            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Ensuring the remote checkout is up to date.",
            )? {
                return Ok(());
            }
            let fork_git_url = EnsureCheckoutAction::new(
                &ssh_client,
                &project_metadata,
                &repository_name,
                &checkout_path,
                &github_login,
            )
            .execute()?;

            let mut updated_registry = remote_registry;
            updated_registry.projects.insert(
                task.project.clone(),
                RemoteProjectRegistryEntry {
                    checkout_path: checkout_path.clone(),
                    fork_git_url: fork_git_url.clone(),
                    repo_url: project_metadata.repo_url.clone(),
                    git_url: project_metadata.git_url.clone(),
                    base_branch: project_metadata.base_branch.clone(),
                    updated_at: format_iso_8601_millis(now_utc()),
                },
            );
            WriteRemoteRegistryAction::new(
                &ssh_client,
                &remote_agent.projects_registry_path,
                &updated_registry,
            )
            .execute()?;

            if !self.save_preparing_phase(&mut dispatch_record, "Preparing the task worktree.")? {
                return Ok(());
            }
            if dispatch_record.follow_up_request.is_some() {
                EnsureFollowUpWorktreeAction::new(
                    &ssh_client,
                    &checkout_path,
                    &branch_name,
                    &worktree_path,
                )
                .execute()?;
            } else {
                CreateWorktreeAction::new(
                    &ssh_client,
                    &checkout_path,
                    &project_metadata.base_branch,
                    &branch_name,
                    &worktree_path,
                )
                .execute()?;
            }

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
            UploadRemoteFileAction::new(
                &ssh_client,
                &format!("{remote_run_directory}/{REMOTE_PROMPT_FILE_NAME}"),
                &prompt,
            )
            .execute()?;
            UploadRemoteFileAction::new(
                &ssh_client,
                &format!("{remote_run_directory}/{REMOTE_SCHEMA_FILE_NAME}"),
                &schema,
            )
            .execute()?;

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
            LaunchRemoteDispatchAction::new(
                &ssh_client,
                &remote_run_directory,
                &worktree_path,
                dispatch_record.preferred_tool,
            )
            .execute()?;

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

                dispatch_record.status = DispatchStatus::Running;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = None;
                dispatch_record.summary =
                    Some("The remote agent is working in the prepared environment.".to_owned());
                dispatch_record.error_message = None;
                self.dispatch_repository.save_dispatch(&dispatch_record)?;
                Ok(dispatch_record)
            }
            Err(error) => {
                dispatch_record.status = DispatchStatus::Failed;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = Some(dispatch_record.updated_at);
                dispatch_record.error_message = Some(error.to_string());
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
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::DispatchNotFound,
                    format!("Task {task_id} does not have a remote dispatch to cancel."),
                )
            })?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Task {task_id} does not have an active remote dispatch to cancel."),
            ));
        }

        self.cancel_remote_dispatch_if_possible(&latest_dispatch)?;

        latest_dispatch.status = DispatchStatus::Canceled;
        latest_dispatch.updated_at = now_utc();
        latest_dispatch.finished_at = Some(latest_dispatch.updated_at);
        latest_dispatch.summary = Some("Canceled from the web UI.".to_owned());
        latest_dispatch.notes = None;
        latest_dispatch.error_message = None;
        self.dispatch_repository.save_dispatch(&latest_dispatch)?;

        Ok(latest_dispatch)
    }

    pub fn discard_dispatch_history(&self, task_id: &str) -> Result<(), TrackError> {
        let latest_dispatch = self
            .dispatch_repository
            .latest_dispatch_for_task(task_id)?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::DispatchNotFound,
                    format!("Task {task_id} does not have a remote dispatch to discard."),
                )
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
        CancelRemoteDispatchAction::new(&ssh_client, &remote_run_directory).execute()
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

        dispatch_record.status = DispatchStatus::Preparing;
        dispatch_record.summary = Some(summary.to_owned());
        dispatch_record.updated_at = now_utc();
        dispatch_record.finished_at = None;
        dispatch_record.error_message = None;
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
