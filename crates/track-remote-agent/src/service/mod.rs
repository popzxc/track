use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, OnceLock};

use serde::de::DeserializeOwned;
use track_config::paths::{collapse_home_path, path_to_string};
use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::task_description::append_follow_up_request;
use track_types::task_id::build_unique_task_id;
use track_types::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_seconds};
use track_types::types::{
    CreateReviewInput, DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentPreferredTool,
    RemoteAgentReviewOutcome, RemoteCleanupSummary, RemoteResetSummary, ReviewRecord,
    ReviewRunRecord, Status, Task, TaskDispatchRecord, TaskUpdateInput,
};

use crate::constants::{
    PREPARING_STALE_AFTER, REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME,
    REVIEW_RUN_DIRECTORY_NAME, REVIEW_WORKTREE_DIRECTORY_NAME,
};
use crate::scripts::{
    render_remote_script_with_shell_prelude, CancelRemoteDispatchScript,
    CleanupOrphanedRemoteArtifactsScript, CleanupReviewArtifactsScript,
    CleanupReviewWorkspaceCachesScript, CleanupTaskArtifactsScript, CreateReviewWorktreeScript,
    CreateWorktreeScript, EnsureCheckoutScript, EnsureFollowUpWorktreeScript, FetchGithubApiScript,
    FetchGithubLoginScript, LaunchRemoteDispatchScript, PostPullRequestCommentScript,
    PrepareRemoteUploadScript, ReadDispatchSnapshotsScript, ReadRemoteFileScript,
    RemoteAgentLauncherScript, ResetWorkspaceScript,
};
use crate::types::{
    ClaudeStructuredOutputEnvelope, GithubPullRequestApiResponse, GithubPullRequestMetadata,
    GithubPullRequestReference, GithubPullRequestReviewState, GithubReviewApiResponse,
    GithubSubmittedReview, RemoteArtifactCleanupCounts, RemoteDispatchSnapshot,
    RemoteProjectRegistryEntry, RemoteProjectRegistryFile, RemoteReviewFollowUpReconciliation,
    RemoteTaskCleanupMode,
};
use crate::utils::{
    build_remote_dispatch_prompt, build_remote_dispatch_schema, build_remote_review_prompt,
    build_remote_review_schema, build_review_follow_up_notification_comment,
    build_review_follow_up_request, build_review_workspace_key, contextualize_track_error,
    describe_remote_reset_blockers, parse_github_pull_request_reference,
    parse_github_repository_name, review_follow_up_event, unique_review_run_directories,
    unique_review_worktree_paths,
};

pub trait RemoteAgentConfigProvider {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError>;
}

type RemoteAgentConfigService = dyn RemoteAgentConfigProvider;

impl<T: RemoteAgentConfigProvider + ?Sized> RemoteAgentConfigProvider for std::sync::Arc<T> {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        (**self).load_remote_agent_runtime_config()
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct StaticRemoteAgentConfigService {
    remote_agent: Option<RemoteAgentRuntimeConfig>,
}

#[cfg(test)]
impl StaticRemoteAgentConfigService {
    pub(crate) fn new(remote_agent: Option<RemoteAgentRuntimeConfig>) -> Self {
        Self { remote_agent }
    }
}

#[cfg(test)]
impl RemoteAgentConfigProvider for StaticRemoteAgentConfigService {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        Ok(self.remote_agent.clone())
    }
}

#[derive(Debug, Default)]
struct TaskDispatchStartGate {
    active_task_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
struct TaskDispatchStartGuard {
    task_id: String,
}

impl TaskDispatchStartGuard {
    fn acquire(task_id: &str) -> Self {
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

#[derive(Debug, Default)]
struct ReviewDispatchStartGate {
    active_review_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
struct ReviewDispatchStartGuard {
    review_id: String,
}

impl ReviewDispatchStartGuard {
    fn acquire(review_id: &str) -> Self {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");

        while active_review_ids.contains(review_id) {
            active_review_ids = gate
                .wake_waiters
                .wait(active_review_ids)
                .expect("review dispatch start gate should not be poisoned");
        }

        active_review_ids.insert(review_id.to_owned());

        Self {
            review_id: review_id.to_owned(),
        }
    }
}

impl Drop for ReviewDispatchStartGuard {
    fn drop(&mut self) {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");
        active_review_ids.remove(&self.review_id);
        gate.wake_waiters.notify_all();
    }
}

fn review_dispatch_start_gate() -> &'static ReviewDispatchStartGate {
    static GATE: OnceLock<ReviewDispatchStartGate> = OnceLock::new();

    // Reviews are now follow-up capable, so the same "check for active work,
    // then persist a preparing record" race that tasks already guard against
    // applies here too. Keeping a dedicated gate per review id preserves the
    // review domain boundary without forcing the task flow to share review-only
    // coordination state.
    GATE.get_or_init(ReviewDispatchStartGate::default)
}

pub struct RemoteDispatchService<'a> {
    pub config_service: &'a RemoteAgentConfigService,
    pub dispatch_repository: &'a DispatchRepository,
    pub project_repository: &'a ProjectRepository,
    pub task_repository: &'a FileTaskRepository,
    pub review_repository: &'a ReviewRepository,
    pub review_dispatch_repository: &'a ReviewDispatchRepository,
}

pub struct RemoteReviewService<'a> {
    pub config_service: &'a RemoteAgentConfigService,
    pub project_repository: &'a ProjectRepository,
    pub review_repository: &'a ReviewRepository,
    pub review_dispatch_repository: &'a ReviewDispatchRepository,
}

impl<'a> RemoteDispatchService<'a> {
    fn review_service(&self) -> RemoteReviewService<'_> {
        RemoteReviewService {
            config_service: self.config_service,
            project_repository: self.project_repository,
            review_repository: self.review_repository,
            review_dispatch_repository: self.review_dispatch_repository,
        }
    }

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
                load_remote_registry(&ssh_client, &remote_agent.projects_registry_path)?;
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Checking GitHub authentication on the remote machine.",
            )? {
                return Ok(());
            }
            let github_login = ssh_client.fetch_github_login()?;
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
            let fork_git_url = ssh_client.ensure_checkout(
                &project_metadata,
                &repository_name,
                &checkout_path,
                &github_login,
            )?;

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
            write_remote_registry(
                &ssh_client,
                &remote_agent.projects_registry_path,
                &updated_registry,
            )?;

            if !self.save_preparing_phase(&mut dispatch_record, "Preparing the task worktree.")? {
                return Ok(());
            }
            if dispatch_record.follow_up_request.is_some() {
                ssh_client.ensure_follow_up_worktree(
                    &checkout_path,
                    &branch_name,
                    &worktree_path,
                )?;
            } else {
                ssh_client.create_worktree(
                    &checkout_path,
                    &project_metadata.base_branch,
                    &branch_name,
                    &worktree_path,
                )?;
            }

            let prompt = build_remote_dispatch_prompt(
                &task.project,
                &project_metadata,
                &branch_name,
                &worktree_path,
                &task.description,
                dispatch_record.pull_request_url.as_deref(),
                dispatch_record.follow_up_request.as_deref(),
            );
            let schema = build_remote_dispatch_schema();
            if !self.save_preparing_phase(
                &mut dispatch_record,
                "Uploading the agent prompt and schema.",
            )? {
                return Ok(());
            }
            ssh_client.upload_remote_file(
                &format!("{remote_run_directory}/{REMOTE_PROMPT_FILE_NAME}"),
                &prompt,
            )?;
            ssh_client.upload_remote_file(
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
            ssh_client.launch_remote_dispatch(
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

        let orphan_cleanup_counts = ssh_client.cleanup_orphaned_remote_artifacts(
            &remote_agent.workspace_root,
            &kept_worktree_paths.into_iter().collect::<Vec<_>>(),
            &kept_run_directories.into_iter().collect::<Vec<_>>(),
        )?;
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
        ssh_client.reset_workspace(
            &remote_agent.workspace_root,
            &remote_agent.projects_registry_path,
        )
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
    // Review Follow-Up Reconciliation
    // =============================================================================
    //
    // The review automation stays intentionally narrow for now:
    //
    // - mention the configured `mainUser` on the PR after a PR head changes
    // - queue one follow-up when that same user leaves actionable review feedback
    //
    // We treat review submissions as "newer than the bot run" when their
    // timestamp is after the dispatch `created_at`. That is conservative on
    // purpose: if review feedback lands while a bot run is already active, we
    // would rather schedule one extra follow-up than silently miss the human
    // feedback entirely.
    pub fn reconcile_review_follow_up(
        &self,
    ) -> Result<RemoteReviewFollowUpReconciliation, TrackError> {
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
                return Ok(RemoteReviewFollowUpReconciliation::default());
            }
            Err(error) => return Err(error),
        };
        let Some(remote_agent) = remote_agent else {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        };
        let Some(review_follow_up) = remote_agent.review_follow_up.clone() else {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        };
        if !remote_agent.managed_key_path.exists() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let task_ids = self.dispatch_repository.task_ids_with_history()?;
        if task_ids.is_empty() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let latest_dispatches = self.latest_dispatches_for_tasks(&task_ids)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let mut reconciliation = RemoteReviewFollowUpReconciliation::default();

        for dispatch_record in latest_dispatches {
            let Some(pull_request_url) = dispatch_record
                .pull_request_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };

            match self.task_repository.get_task(&dispatch_record.task_id) {
                Ok(task) if task.status == Status::Open => task,
                Ok(_) => continue,
                Err(error) if error.code == ErrorCode::TaskNotFound => continue,
                Err(error) => return Err(error),
            };

            let pull_request_state = ssh_client
                .fetch_pull_request_review_state(pull_request_url, &review_follow_up.main_user)
                .map_err(|error| {
                    contextualize_track_error(
                        error,
                        format!(
                            "Review follow-up could not inspect task {} PR {} for reviewer @{}",
                            dispatch_record.task_id, pull_request_url, review_follow_up.main_user
                        ),
                    )
                });
            let pull_request_state = match pull_request_state {
                Ok(pull_request_state) => pull_request_state,
                Err(error) => {
                    reconciliation.failures += 1;
                    reconciliation.events.push(review_follow_up_event(
                        "fetch_failed",
                        error.to_string(),
                        &dispatch_record,
                        &review_follow_up.main_user,
                        None,
                    ));
                    continue;
                }
            };

            reconciliation.events.push(review_follow_up_event(
                "task_evaluated",
                "Fetched PR review state for automatic follow-up reconciliation.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            if !pull_request_state.is_open {
                reconciliation.events.push(review_follow_up_event(
                    "skip_closed_pr",
                    "Skipped automatic follow-up because the PR is not open anymore.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            if dispatch_record.status.is_active() {
                reconciliation.events.push(review_follow_up_event(
                    "skip_active_dispatch",
                    "Skipped automatic follow-up because the latest dispatch is still active.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            if let Some(latest_review) = pull_request_state.latest_eligible_review.as_ref() {
                if latest_review.submitted_at > dispatch_record.created_at {
                    let follow_up_request = build_review_follow_up_request(
                        pull_request_url,
                        &review_follow_up.main_user,
                        dispatch_record.created_at,
                    );
                    let queued_dispatch = self
                        .queue_follow_up_dispatch(&dispatch_record.task_id, &follow_up_request)?;
                    reconciliation.events.push(review_follow_up_event(
                        "queue_follow_up",
                        format!(
                            "Queued a follow-up dispatch because reviewer @{} submitted {} at {} after dispatch {} started.",
                            review_follow_up.main_user,
                            latest_review.state,
                            format_iso_8601_millis(latest_review.submitted_at),
                            dispatch_record.dispatch_id,
                        ),
                        &queued_dispatch,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    reconciliation.queued_dispatches.push(queued_dispatch);
                    continue;
                }
            }

            if pull_request_state.head_oid.is_empty() {
                reconciliation.events.push(review_follow_up_event(
                    "skip_missing_head_oid",
                    "Skipped PR reviewer notification because the PR head SHA is missing.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            let already_recorded_for_head = dispatch_record.review_request_head_oid.as_deref()
                == Some(pull_request_state.head_oid.as_str())
                && dispatch_record.review_request_user.as_deref()
                    == Some(review_follow_up.main_user.as_str());
            if already_recorded_for_head {
                reconciliation.events.push(review_follow_up_event(
                    "skip_notification_already_recorded",
                    "Skipped PR reviewer notification because this PR head already recorded the same reviewer.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            let notification_comment = build_review_follow_up_notification_comment(
                &review_follow_up.main_user,
                &pull_request_state.head_oid,
            );
            let notify_reviewer_result = ssh_client
                .post_pull_request_comment(pull_request_url, &notification_comment)
                .map_err(|error| {
                    contextualize_track_error(
                        error,
                        format!(
                            "Review follow-up could not notify reviewer @{} for task {} PR {}",
                            review_follow_up.main_user, dispatch_record.task_id, pull_request_url
                        ),
                    )
                });
            if let Err(error) = notify_reviewer_result {
                reconciliation.failures += 1;
                reconciliation.events.push(review_follow_up_event(
                    "notify_reviewer_failed",
                    error.to_string(),
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }
            self.mark_review_notification_for_head(
                &dispatch_record,
                &pull_request_state.head_oid,
                &review_follow_up.main_user,
            )?;
            reconciliation.events.push(review_follow_up_event(
                "notify_reviewer_posted",
                "Posted a PR comment mentioning the configured main GitHub user for the current PR head.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            reconciliation.review_notifications_updated += 1;
        }

        Ok(reconciliation)
    }

    fn refresh_active_dispatch_records(
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
        ssh_client.cancel_remote_dispatch(&remote_run_directory)
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

    fn mark_review_notification_for_head(
        &self,
        dispatch_record: &TaskDispatchRecord,
        head_oid: &str,
        review_user: &str,
    ) -> Result<(), TrackError> {
        let mut updated_record = dispatch_record.clone();

        // These persisted field names started out as "review request" markers.
        // We intentionally keep them for backward compatibility with existing
        // dispatch JSON while reusing them as a "notified this reviewer about
        // this PR head already" checkpoint.
        updated_record.review_request_head_oid = Some(head_oid.to_owned());
        updated_record.review_request_user = Some(review_user.to_owned());
        self.dispatch_repository.save_dispatch(&updated_record)
    }

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

        ssh_client.cleanup_task_artifacts(
            &checkout_path,
            &worktree_paths,
            &run_directories,
            cleanup_mode,
        )
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

    fn finalize_dispatch_locally(
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
            load_remote_registry(ssh_client, &remote_agent.projects_registry_path)?;
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

        ssh_client.cleanup_review_workspace_caches(&checkout_paths)?;

        let mut registry_changed = false;
        for workspace_key in workspace_keys {
            registry_changed |= remote_registry.projects.remove(workspace_key).is_some();
        }

        if registry_changed {
            write_remote_registry(
                ssh_client,
                &remote_agent.projects_registry_path,
                &remote_registry,
            )?;
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
            load_remote_registry(ssh_client, &remote_agent.projects_registry_path)?;

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

impl<'a> RemoteReviewService<'a> {
    // =============================================================================
    // Review Request Entry Points
    // =============================================================================
    //
    // Reviews are intentionally a separate domain from task dispatches. They
    // still reuse the same remote runner and SSH bootstrap, but they start
    // from a PR URL, persist their own local records, and ask the agent to
    // submit a GitHub review directly instead of creating or updating a PR
    // branch.
    pub fn create_review(
        &self,
        input: CreateReviewInput,
    ) -> Result<(ReviewRecord, ReviewRunRecord), TrackError> {
        let validated_input = input.validate()?;
        let (remote_agent, review_settings) = self.load_review_runtime_prerequisites()?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            ssh_client.fetch_pull_request_metadata(&validated_input.pull_request_url)?;
        let initial_target_head_oid = pull_request_metadata.head_oid.clone();
        let project_match = self
            .project_repository
            .list_projects()?
            .into_iter()
            .find(|project| project.metadata.repo_url.trim() == pull_request_metadata.repo_url);
        let project_metadata_override = project_match
            .as_ref()
            .map(|project| project.metadata.clone());
        let workspace_key = project_match
            .as_ref()
            .map(|project| project.canonical_name.clone())
            .unwrap_or_else(|| build_review_workspace_key(&pull_request_metadata));
        let review_timestamp = now_utc();
        let review_id = build_unique_task_id(
            review_timestamp,
            &format!(
                "review {} pr {}",
                pull_request_metadata.repository_full_name,
                pull_request_metadata.pull_request_number
            ),
            |candidate| self.review_repository.get_review(candidate).is_ok(),
        );
        let review = ReviewRecord {
            id: review_id,
            pull_request_url: pull_request_metadata.pull_request_url,
            pull_request_number: pull_request_metadata.pull_request_number,
            pull_request_title: pull_request_metadata.pull_request_title,
            repository_full_name: pull_request_metadata.repository_full_name,
            repo_url: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.repo_url.clone())
                .unwrap_or(pull_request_metadata.repo_url),
            git_url: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.git_url.clone())
                .unwrap_or(pull_request_metadata.git_url),
            base_branch: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.base_branch.clone())
                .unwrap_or(pull_request_metadata.base_branch),
            workspace_key,
            preferred_tool: validated_input
                .preferred_tool
                .unwrap_or(remote_agent.preferred_tool),
            project: project_match.map(|project| project.canonical_name),
            main_user: review_settings.main_user,
            default_review_prompt: review_settings.default_review_prompt,
            extra_instructions: validated_input.extra_instructions,
            created_at: review_timestamp,
            updated_at: review_timestamp,
        };

        self.review_repository.save_review(&review)?;
        match self.queue_review_dispatch(
            &review,
            &remote_agent,
            None,
            Some(initial_target_head_oid.as_str()),
        ) {
            Ok(dispatch) => Ok((review, dispatch)),
            Err(error) => {
                let _ = self.review_repository.delete_review(&review.id);
                Err(error)
            }
        }
    }

    // =============================================================================
    // Follow-Up Review Runs
    // =============================================================================
    //
    // A re-review should feel like the PR equivalent of a task follow-up: the
    // saved review record remains the durable anchor, while each new run stores
    // the latest user ask plus the exact PR head it targeted. We deliberately
    // fetch fresh PR metadata here so each run records which commit the agent
    // reviewed instead of assuming the PR stayed on the same head as the
    // initial request.
    pub fn queue_follow_up_review_dispatch(
        &self,
        review_id: &str,
        follow_up_request: &str,
    ) -> Result<ReviewRunRecord, TrackError> {
        let trimmed_follow_up_request = follow_up_request.trim();
        if trimmed_follow_up_request.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a re-review request for the remote agent.",
            ));
        }

        let (remote_agent, mut review) = self.load_review_dispatch_prerequisites(review_id)?;
        let _dispatch_start_guard = ReviewDispatchStartGuard::acquire(review_id);
        self.ensure_no_blocking_active_review_dispatch(review_id)?;

        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            ssh_client.fetch_pull_request_metadata(&review.pull_request_url)?;
        let previous_updated_at = review.updated_at;
        review.updated_at = now_utc();
        self.review_repository.save_review(&review)?;

        match self.queue_review_dispatch(
            &review,
            &remote_agent,
            Some(trimmed_follow_up_request),
            Some(pull_request_metadata.head_oid.as_str()),
        ) {
            Ok(dispatch) => Ok(dispatch),
            Err(error) => {
                review.updated_at = previous_updated_at;
                let _ = self.review_repository.save_review(&review);
                Err(error)
            }
        }
    }

    pub fn launch_prepared_review(
        &self,
        mut dispatch_record: ReviewRunRecord,
    ) -> Result<ReviewRunRecord, TrackError> {
        if let Some(existing_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)?
        {
            if !existing_record.status.is_active() {
                return Ok(existing_record);
            }
        }

        let worktree_path = dispatch_record
            .worktree_path
            .clone()
            .expect("queued review dispatches should store a worktree path");
        let branch_name = dispatch_record
            .branch_name
            .clone()
            .expect("queued review dispatches should store a branch name");
        let remote_run_directory =
            derive_review_run_directory(&worktree_path, &dispatch_record.dispatch_id)?;

        let launch_result = (|| -> Result<(), TrackError> {
            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Checking remote review prerequisites.",
            )? {
                return Ok(());
            }
            let (remote_agent, review) =
                self.load_review_dispatch_prerequisites(&dispatch_record.review_id)?;
            let ssh_client = SshClient::new(&remote_agent)?;

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Loading the remote project registry.",
            )? {
                return Ok(());
            }
            let remote_registry =
                load_remote_registry(&ssh_client, &remote_agent.projects_registry_path)?;

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Checking GitHub authentication on the remote machine.",
            )? {
                return Ok(());
            }
            let github_login = ssh_client.fetch_github_login()?;
            let repository_name = parse_github_repository_name(&review.repo_url)?;
            let checkout_path = remote_registry
                .projects
                .get(&review.workspace_key)
                .map(|entry| entry.checkout_path.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{}/{}/{}",
                        remote_agent.workspace_root.trim_end_matches('/'),
                        review.workspace_key,
                        review.workspace_key
                    )
                });

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Ensuring the remote checkout is up to date.",
            )? {
                return Ok(());
            }
            let fork_git_url = ssh_client.ensure_checkout(
                &ProjectMetadata {
                    repo_url: review.repo_url.clone(),
                    git_url: review.git_url.clone(),
                    base_branch: review.base_branch.clone(),
                    description: None,
                },
                &repository_name,
                &checkout_path,
                &github_login,
            )?;

            let mut updated_registry = remote_registry;
            updated_registry.projects.insert(
                review.workspace_key.clone(),
                RemoteProjectRegistryEntry {
                    checkout_path: checkout_path.clone(),
                    fork_git_url,
                    repo_url: review.repo_url.clone(),
                    git_url: review.git_url.clone(),
                    base_branch: review.base_branch.clone(),
                    updated_at: format_iso_8601_millis(now_utc()),
                },
            );
            write_remote_registry(
                &ssh_client,
                &remote_agent.projects_registry_path,
                &updated_registry,
            )?;

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Preparing the review worktree.",
            )? {
                return Ok(());
            }
            ssh_client.create_review_worktree(
                &checkout_path,
                review.pull_request_number,
                &branch_name,
                &worktree_path,
                dispatch_record.target_head_oid.as_deref(),
            )?;

            let dispatch_history = self
                .review_dispatch_repository
                .dispatches_for_review(&review.id)?;
            let previous_submitted_review = select_previous_submitted_review_run(
                &dispatch_history,
                &dispatch_record.dispatch_id,
            );
            let prompt =
                build_remote_review_prompt(&review, &dispatch_record, previous_submitted_review);
            let schema = build_remote_review_schema();
            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Uploading the review prompt and schema.",
            )? {
                return Ok(());
            }
            ssh_client.upload_remote_file(
                &format!("{remote_run_directory}/{REMOTE_PROMPT_FILE_NAME}"),
                &prompt,
            )?;
            ssh_client.upload_remote_file(
                &format!("{remote_run_directory}/{REMOTE_SCHEMA_FILE_NAME}"),
                &schema,
            )?;

            if !self.dispatch_is_still_active(
                &dispatch_record.review_id,
                &dispatch_record.dispatch_id,
            )? {
                return Ok(());
            }

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Launching the remote review agent.",
            )? {
                return Ok(());
            }
            ssh_client.launch_remote_dispatch(
                &remote_run_directory,
                &worktree_path,
                dispatch_record.preferred_tool,
            )?;

            Ok(())
        })();

        match launch_result {
            Ok(()) => {
                if let Some(existing_record) = self.load_saved_review_dispatch(
                    &dispatch_record.review_id,
                    &dispatch_record.dispatch_id,
                )? {
                    if !existing_record.status.is_active() {
                        let _ = self.cancel_remote_review_if_possible(&existing_record);
                        return Ok(existing_record);
                    }
                }

                dispatch_record.status = DispatchStatus::Running;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = None;
                dispatch_record.summary =
                    Some("The remote agent is reviewing the prepared pull request.".to_owned());
                dispatch_record.error_message = None;
                self.review_dispatch_repository
                    .save_dispatch(&dispatch_record)?;
                Ok(dispatch_record)
            }
            Err(error) => {
                dispatch_record.status = DispatchStatus::Failed;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = Some(dispatch_record.updated_at);
                dispatch_record.error_message = Some(error.to_string());
                self.review_dispatch_repository
                    .save_dispatch(&dispatch_record)?;
                Err(error)
            }
        }
    }

    pub fn latest_dispatches_for_reviews(
        &self,
        review_ids: &[String],
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = Vec::new();
        for review_id in review_ids {
            if let Some(record) = self
                .review_dispatch_repository
                .latest_dispatch_for_review(review_id)?
            {
                records.push(record);
            }
        }

        self.refresh_active_review_dispatch_records(records)
    }

    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self.review_dispatch_repository.list_dispatches(limit)?;
        self.refresh_active_review_dispatch_records(records)
    }

    pub fn dispatch_history_for_review(
        &self,
        review_id: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = self
            .review_dispatch_repository
            .dispatches_for_review(review_id)?;
        if records
            .first()
            .is_some_and(|record| record.status.is_active())
        {
            if let Some(refreshed_latest) = self
                .latest_dispatches_for_reviews(&[review_id.to_owned()])?
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

    pub fn cancel_dispatch(&self, review_id: &str) -> Result<ReviewRunRecord, TrackError> {
        let mut latest_dispatch = self
            .latest_dispatches_for_reviews(&[review_id.to_owned()])?
            .into_iter()
            .next()
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::DispatchNotFound,
                    format!("Review {review_id} does not have a remote run to cancel."),
                )
            })?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Review {review_id} does not have an active remote run to cancel."),
            ));
        }

        self.cancel_remote_review_if_possible(&latest_dispatch)?;

        latest_dispatch.status = DispatchStatus::Canceled;
        latest_dispatch.updated_at = now_utc();
        latest_dispatch.finished_at = Some(latest_dispatch.updated_at);
        latest_dispatch.summary = Some("Canceled from the web UI.".to_owned());
        latest_dispatch.notes = None;
        latest_dispatch.error_message = None;
        self.review_dispatch_repository
            .save_dispatch(&latest_dispatch)?;

        Ok(latest_dispatch)
    }

    fn ensure_no_blocking_active_review_dispatch(&self, review_id: &str) -> Result<(), TrackError> {
        if let Some(existing_dispatch) = self
            .latest_dispatches_for_reviews(&[review_id.to_owned()])?
            .into_iter()
            .next()
            .filter(|record| record.status.is_active())
        {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Review {review_id} already has an active remote run ({})",
                    existing_dispatch.dispatch_id
                ),
            ));
        }

        Ok(())
    }

    pub fn delete_review(&self, review_id: &str) -> Result<(), TrackError> {
        let review = self.review_repository.get_review(review_id)?;
        let dispatch_history = self
            .review_dispatch_repository
            .dispatches_for_review(review_id)?;
        if !dispatch_history.is_empty() {
            if let Err(error) = self.cleanup_review_remote_artifacts(&review, &dispatch_history) {
                eprintln!("Skipping remote cleanup while deleting review {review_id}: {error}");
            }

            self.review_dispatch_repository
                .delete_dispatch_history_for_review(review_id)?;
        }

        self.review_repository.delete_review(review_id)
    }

    fn queue_review_dispatch(
        &self,
        review: &ReviewRecord,
        remote_agent: &RemoteAgentRuntimeConfig,
        follow_up_request: Option<&str>,
        target_head_oid: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let mut dispatch_record = self.review_dispatch_repository.create_dispatch(
            review,
            &remote_agent.host,
            review.preferred_tool,
        )?;
        dispatch_record.branch_name = Some(format!("track-review/{}", dispatch_record.dispatch_id));
        dispatch_record.worktree_path = Some(format!(
            "{}/{}/{}/{}",
            remote_agent.workspace_root.trim_end_matches('/'),
            review.workspace_key,
            REVIEW_WORKTREE_DIRECTORY_NAME,
            dispatch_record.dispatch_id
        ));
        dispatch_record.follow_up_request = follow_up_request.map(str::trim).map(ToOwned::to_owned);
        dispatch_record.target_head_oid = target_head_oid
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(follow_up_request) = dispatch_record.follow_up_request.as_deref() {
            dispatch_record.summary = Some(format!(
                "Re-review request: {}",
                first_follow_up_line(follow_up_request)
            ));
        }
        dispatch_record.updated_at = now_utc();
        self.review_dispatch_repository
            .save_dispatch(&dispatch_record)?;

        Ok(dispatch_record)
    }

    fn refresh_active_review_dispatch_records(
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

    fn refresh_review_dispatch_record_from_snapshot(
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
            let outcome = parse_remote_agent_output::<RemoteAgentReviewOutcome>(
                remote_result,
                record.preferred_tool,
                "Remote review result",
            )?;

            // The review agent now owns the GitHub side effect directly, just
            // like the task flow owns its own pushes and PR creation. We keep
            // the local record intentionally narrow here: enough structured
            // metadata for status/history, without mirroring GitHub discussion
            // threads back into local storage.
            // TODO: If the web UI needs first-class inline-comment history,
            // persist dedicated review metadata rather than trying to mirror
            // every GitHub thread inside this local run record.
            // TODO: If we need to reconcile "review submitted, final JSON not
            // written" crashes, capture the GitHub review handle in a sidecar
            // file during the remote run before the final structured result.
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

    fn finalize_review_dispatch_locally(
        &self,
        dispatch_record: &ReviewRunRecord,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let mut updated_record = dispatch_record.clone();
        let now = now_utc();
        updated_record.status = status;
        updated_record.updated_at = now;
        updated_record.finished_at = Some(now);
        updated_record.summary = Some(summary.to_owned());
        updated_record.notes = None;
        updated_record.error_message = error_message.map(ToOwned::to_owned);
        self.review_dispatch_repository
            .save_dispatch(&updated_record)?;

        Ok(updated_record)
    }

    fn load_saved_review_dispatch(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        self.review_dispatch_repository
            .get_dispatch(review_id, dispatch_id)
    }

    fn dispatch_is_still_active(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<bool, TrackError> {
        Ok(self
            .load_saved_review_dispatch(review_id, dispatch_id)?
            .map(|record| record.status.is_active())
            .unwrap_or(false))
    }

    fn save_review_preparing_phase(
        &self,
        dispatch_record: &mut ReviewRunRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)?
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
        self.review_dispatch_repository
            .save_dispatch(dispatch_record)?;

        Ok(true)
    }

    fn cancel_remote_review_if_possible(
        &self,
        dispatch_record: &ReviewRunRecord,
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
            derive_review_run_directory(worktree_path, &dispatch_record.dispatch_id)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        ssh_client.cancel_remote_dispatch(&remote_run_directory)
    }

    fn cleanup_review_remote_artifacts(
        &self,
        review: &ReviewRecord,
        dispatch_history: &[ReviewRunRecord],
    ) -> Result<(), TrackError> {
        if dispatch_history.is_empty() {
            return Ok(());
        }

        let remote_agent = self.load_remote_agent_for_review_cleanup(&review.id)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let checkout_path =
            self.resolve_review_checkout_path(&ssh_client, &remote_agent, &review.workspace_key)?;
        let worktree_paths = unique_review_worktree_paths(dispatch_history);
        let run_directories = unique_review_run_directories(dispatch_history, &remote_agent);
        let branch_names = dispatch_history
            .iter()
            .filter_map(|record| record.branch_name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        ssh_client.cleanup_review_artifacts(
            &checkout_path,
            &branch_names,
            &worktree_paths,
            &run_directories,
        )
    }

    fn load_remote_agent_for_review_cleanup(
        &self,
        review_id: &str,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Review {review_id} has remote history, but remote-agent configuration is missing so cleanup cannot run."
                ),
            )
        })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before cleaning review {review_id}.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        Ok(remote_agent)
    }

    fn resolve_review_checkout_path(
        &self,
        ssh_client: &SshClient,
        remote_agent: &RemoteAgentRuntimeConfig,
        workspace_key: &str,
    ) -> Result<String, TrackError> {
        let remote_registry =
            load_remote_registry(ssh_client, &remote_agent.projects_registry_path)?;

        Ok(remote_registry
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
            }))
    }

    // =============================================================================
    // Review Runner Prerequisites
    // =============================================================================
    //
    // Saved reviews snapshot the review-specific knobs they need for future
    // re-reviews, namely the main GitHub user and default prompt. That means
    // later follow-up runs should only depend on the remote runner itself still
    // being usable, not on the mutable global review-follow-up block still
    // existing in the current config.
    fn load_review_runner_prerequisites(&self) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
            .ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote reviews are not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
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

        Ok(remote_agent)
    }

    fn load_review_runtime_prerequisites(
        &self,
    ) -> Result<
        (
            RemoteAgentRuntimeConfig,
            RemoteAgentReviewFollowUpRuntimeConfig,
        ),
        TrackError,
    > {
        let remote_agent = self.load_review_runner_prerequisites()?;
        let review_settings = remote_agent.review_follow_up.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "PR reviews require a configured main GitHub user in the remote runner settings.",
            )
        })?;

        Ok((remote_agent, review_settings))
    }

    fn load_review_dispatch_prerequisites(
        &self,
        review_id: &str,
    ) -> Result<(RemoteAgentRuntimeConfig, ReviewRecord), TrackError> {
        let remote_agent = self.load_review_runner_prerequisites()?;
        let review = self.review_repository.get_review(review_id)?;

        Ok((remote_agent, review))
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

fn select_follow_up_base_dispatch(
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

fn select_previous_submitted_review_run<'a>(
    dispatch_history: &'a [ReviewRunRecord],
    current_dispatch_id: &str,
) -> Option<&'a ReviewRunRecord> {
    dispatch_history.iter().find(|record| {
        record.dispatch_id != current_dispatch_id
            && record.review_submitted
            && (record.github_review_url.is_some() || record.github_review_id.is_some())
    })
}

fn latest_pull_request_for_branch(
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
// Dispatch Refresh
// =============================================================================
//
// Dispatches usually run for minutes rather than seconds, so the API optimizes
// for fewer SSH handshakes instead of ultra-fresh status. We batch all active
// dispatch lookups into one SSH round-trip per poll cycle so multiple running
// jobs do not multiply connection setup overhead.
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

    let snapshots = ssh_client.read_dispatch_snapshots(&run_directories)?;
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

fn load_review_snapshots_for_records(
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

    let snapshots = ssh_client.read_dispatch_snapshots(&run_directories)?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}

fn derive_review_run_directory(
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

fn refresh_dispatch_record_from_snapshot(
    mut record: TaskDispatchRecord,
    snapshot: &RemoteDispatchSnapshot,
) -> Result<TaskDispatchRecord, TrackError> {
    let remote_status = snapshot.status.as_deref().unwrap_or_default();
    let remote_status = remote_status.trim();
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
        let outcome = parse_remote_agent_output::<RemoteAgentDispatchOutcome>(
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

fn mark_abandoned_preparing_dispatch(mut record: TaskDispatchRecord) -> Option<TaskDispatchRecord> {
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

fn mark_abandoned_preparing_review_dispatch(
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

fn parse_remote_finished_at(
    value: Option<&str>,
    fallback: time::OffsetDateTime,
) -> time::OffsetDateTime {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| parse_iso_8601_seconds(value).ok())
        .unwrap_or(fallback)
}

fn mark_terminal_refresh_failure(
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

fn mark_terminal_review_refresh_failure(
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

fn load_remote_registry(
    ssh_client: &SshClient,
    registry_path: &str,
) -> Result<RemoteProjectRegistryFile, TrackError> {
    let Some(raw_registry) = ssh_client.read_remote_file(registry_path)? else {
        return Ok(RemoteProjectRegistryFile::default());
    };

    serde_json::from_str::<RemoteProjectRegistryFile>(&raw_registry).map_err(|error| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Remote projects registry is not valid JSON: {error}"),
        )
    })
}

fn write_remote_registry(
    ssh_client: &SshClient,
    registry_path: &str,
    registry: &RemoteProjectRegistryFile,
) -> Result<(), TrackError> {
    let serialized = serde_json::to_string_pretty(registry).map_err(|error| {
        TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Could not serialize the remote projects registry: {error}"),
        )
    })?;
    ssh_client.upload_remote_file(registry_path, &serialized)
}

fn github_pull_request_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "repos/{}/{}/pulls/{}",
        reference.owner, reference.repository, reference.number
    )
}

fn github_pull_request_reviews_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "{}/reviews?per_page=100",
        github_pull_request_endpoint(reference)
    )
}

fn github_pull_request_issue_comments_endpoint(reference: &GithubPullRequestReference) -> String {
    format!(
        "repos/{}/{}/issues/{}/comments",
        reference.owner, reference.repository, reference.number
    )
}

fn parse_remote_agent_output<T>(
    raw_result: &str,
    preferred_tool: RemoteAgentPreferredTool,
    result_label: &str,
) -> Result<T, TrackError>
where
    T: DeserializeOwned,
{
    match serde_json::from_str::<T>(raw_result) {
        Ok(outcome) => Ok(outcome),
        Err(direct_error) if preferred_tool == RemoteAgentPreferredTool::Claude => {
            // Codex writes the structured payload directly, while Claude wraps
            // it in a metadata envelope. Accepting both shapes keeps existing
            // fixtures and any transitional persisted results readable.
            serde_json::from_str::<ClaudeStructuredOutputEnvelope<T>>(raw_result)
                .map(|envelope| envelope.structured_output)
                .map_err(|envelope_error| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        format!(
                            "{result_label} did not match the expected direct or Claude structured-output format: direct parse failed with {direct_error}; envelope parse failed with {envelope_error}",
                        ),
                    )
                })
        }
        Err(error) => Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("{result_label} is not valid JSON: {error}"),
        )),
    }
}

struct SshClient {
    host: String,
    key_path: PathBuf,
    known_hosts_path: PathBuf,
    port: u16,
    shell_prelude: String,
    user: String,
}

impl SshClient {
    fn new(config: &RemoteAgentRuntimeConfig) -> Result<Self, TrackError> {
        if let Some(parent_directory) = config.managed_known_hosts_path.parent() {
            fs::create_dir_all(parent_directory).map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "Could not create the managed known_hosts directory at {}: {error}",
                        collapse_home_path(parent_directory)
                    ),
                )
            })?;
        }

        if !config.managed_known_hosts_path.exists() {
            fs::write(&config.managed_known_hosts_path, "").map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "Could not create the managed known_hosts file at {}: {error}",
                        collapse_home_path(&config.managed_known_hosts_path)
                    ),
                )
            })?;
        }

        Ok(Self {
            host: config.host.clone(),
            key_path: config.managed_key_path.clone(),
            known_hosts_path: config.managed_known_hosts_path.clone(),
            port: config.port,
            shell_prelude: config.shell_prelude.clone().unwrap_or_default(),
            user: config.user.clone(),
        })
    }

    fn fetch_github_login(&self) -> Result<String, TrackError> {
        let script = FetchGithubLoginScript;
        let login = self.run_script(&script.render(), &[])?;

        let login = login.trim().to_owned();
        if login.is_empty() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Remote `gh` authentication did not return a GitHub login.",
            ));
        }

        Ok(login)
    }

    fn fetch_pull_request_metadata(
        &self,
        pull_request_url: &str,
    ) -> Result<GithubPullRequestMetadata, TrackError> {
        let reference = parse_github_pull_request_reference(pull_request_url)?;
        let pull_request_endpoint = github_pull_request_endpoint(&reference);
        let script = FetchGithubApiScript;
        let arguments = script.arguments(&pull_request_endpoint);
        let pull_request_json = self
            .run_script(&script.render(), &arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                        "Remote `gh api` on {}@{} could not fetch PR details for {} via endpoint `{}`",
                        self.user, self.host, pull_request_url, pull_request_endpoint
                    ),
                )
            })?;
        let pull_request =
            serde_json::from_str::<GithubPullRequestApiResponse>(&pull_request_json).map_err(
                |error| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        format!(
                            "GitHub PR details from endpoint `{pull_request_endpoint}` are not valid JSON: {error}"
                        ),
                    )
                },
            )?;

        if pull_request.state != "open" || pull_request.merged_at.is_some() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Pull request {pull_request_url} is not open anymore."),
            ));
        }

        Ok(GithubPullRequestMetadata {
            pull_request_url: pull_request_url.trim().to_owned(),
            pull_request_number: reference.number,
            pull_request_title: pull_request.title,
            repository_full_name: format!("{}/{}", reference.owner, reference.repository),
            repo_url: format!(
                "https://github.com/{}/{}",
                reference.owner, reference.repository
            ),
            git_url: format!(
                "git@github.com:{}/{}.git",
                reference.owner, reference.repository
            ),
            base_branch: pull_request.base.branch_ref,
            head_oid: pull_request.head.sha,
        })
    }

    fn fetch_pull_request_review_state(
        &self,
        pull_request_url: &str,
        main_user: &str,
    ) -> Result<GithubPullRequestReviewState, TrackError> {
        let reference = parse_github_pull_request_reference(pull_request_url)?;
        let pull_request_endpoint = github_pull_request_endpoint(&reference);
        let fetch_api_script = FetchGithubApiScript;
        let pull_request_arguments = fetch_api_script.arguments(&pull_request_endpoint);
        let pull_request_json = self
            .run_script(&fetch_api_script.render(), &pull_request_arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                    "Remote `gh api` on {}@{} could not fetch PR details for {} via endpoint `{}`",
                    self.user, self.host, pull_request_url, pull_request_endpoint
                ),
                )
            })?;
        let pull_request =
            serde_json::from_str::<GithubPullRequestApiResponse>(&pull_request_json).map_err(
                |error| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        format!(
                            "GitHub PR details from endpoint `{pull_request_endpoint}` are not valid JSON: {error}"
                        ),
                    )
                },
            )?;

        let reviews_endpoint = github_pull_request_reviews_endpoint(&reference);
        let review_arguments = fetch_api_script.arguments(&reviews_endpoint);
        let reviews_json = self
            .run_script(&fetch_api_script.render(), &review_arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                    "Remote `gh api` on {}@{} could not fetch PR reviews for {} via endpoint `{}`",
                    self.user, self.host, pull_request_url, reviews_endpoint
                ),
                )
            })?;
        let reviews = serde_json::from_str::<Vec<GithubReviewApiResponse>>(&reviews_json).map_err(
            |error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "GitHub PR reviews from endpoint `{reviews_endpoint}` are not valid JSON: {error}"
                    ),
                )
            },
        )?;

        let latest_eligible_review = reviews
            .into_iter()
            .filter_map(|review| {
                let reviewer = review.user?.login;
                if reviewer != main_user {
                    return None;
                }

                if review.state != "COMMENTED" && review.state != "CHANGES_REQUESTED" {
                    return None;
                }

                let submitted_at = review
                    .submitted_at
                    .as_deref()
                    .and_then(|value| parse_iso_8601_seconds(value).ok())?;

                Some(GithubSubmittedReview {
                    state: review.state,
                    submitted_at,
                })
            })
            .max_by_key(|review| review.submitted_at);

        Ok(GithubPullRequestReviewState {
            is_open: pull_request.state == "open" && pull_request.merged_at.is_none(),
            head_oid: pull_request.head.sha,
            latest_eligible_review,
        })
    }

    fn post_pull_request_comment(
        &self,
        pull_request_url: &str,
        comment_body: &str,
    ) -> Result<(), TrackError> {
        let reference = parse_github_pull_request_reference(pull_request_url)?;
        let issue_comments_endpoint = github_pull_request_issue_comments_endpoint(&reference);
        let script = PostPullRequestCommentScript;
        let arguments = script.arguments(&issue_comments_endpoint, comment_body);
        self.run_script(&script.render(), &arguments)
            .map_err(|error| {
                contextualize_track_error(
                    error,
                    format!(
                    "Remote `gh api` on {}@{} could not post a PR comment for {} via endpoint `{}`",
                    self.user, self.host, pull_request_url, issue_comments_endpoint
                ),
                )
            })?;

        Ok(())
    }

    fn ensure_checkout(
        &self,
        metadata: &ProjectMetadata,
        repository_name: &str,
        checkout_path: &str,
        github_login: &str,
    ) -> Result<String, TrackError> {
        let script = EnsureCheckoutScript;
        let arguments = script.arguments(metadata, repository_name, checkout_path, github_login);
        let fork_git_url = self.run_script(&script.render(), &arguments)?;

        let fork_git_url = fork_git_url.trim().to_owned();
        if fork_git_url.is_empty() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Remote fork setup did not return a fork Git URL.",
            ));
        }

        Ok(fork_git_url)
    }

    fn create_worktree(
        &self,
        checkout_path: &str,
        base_branch: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Result<(), TrackError> {
        let script = CreateWorktreeScript;
        let arguments = script.arguments(checkout_path, base_branch, branch_name, worktree_path);
        self.run_script(&script.render(), &arguments)?;

        Ok(())
    }

    fn create_review_worktree(
        &self,
        checkout_path: &str,
        pull_request_number: u64,
        branch_name: &str,
        worktree_path: &str,
        target_head_oid: Option<&str>,
    ) -> Result<(), TrackError> {
        let script = CreateReviewWorktreeScript;
        let arguments = script.arguments(
            checkout_path,
            pull_request_number,
            branch_name,
            worktree_path,
            target_head_oid,
        );
        self.run_script(&script.render(), &arguments)?;

        Ok(())
    }

    fn ensure_follow_up_worktree(
        &self,
        checkout_path: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Result<(), TrackError> {
        let script = EnsureFollowUpWorktreeScript;
        let arguments = script.arguments(checkout_path, branch_name, worktree_path);
        self.run_script(&script.render(), &arguments)?;

        Ok(())
    }

    fn launch_remote_dispatch(
        &self,
        remote_run_directory: &str,
        worktree_path: &str,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Result<(), TrackError> {
        let launcher_contents =
            RemoteAgentLauncherScript::new(preferred_tool, &self.shell_prelude).render();
        self.upload_remote_file(
            &format!("{remote_run_directory}/launch.sh"),
            &launcher_contents,
        )?;

        let script = LaunchRemoteDispatchScript;
        let arguments = script.arguments(remote_run_directory, worktree_path);
        self.run_script(&script.render(), &arguments)?;

        Ok(())
    }

    fn cancel_remote_dispatch(&self, remote_run_directory: &str) -> Result<(), TrackError> {
        let script = CancelRemoteDispatchScript;
        let arguments = script.arguments(remote_run_directory);
        self.run_script(&script.render(), &arguments)?;
        Ok(())
    }

    fn cleanup_task_artifacts(
        &self,
        checkout_path: &str,
        worktree_paths: &[String],
        run_directories: &[String],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let script = CleanupTaskArtifactsScript::from_mode(cleanup_mode);
        let arguments = script.arguments(checkout_path, worktree_paths, run_directories);
        let report = self.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
    }

    fn cleanup_review_artifacts(
        &self,
        checkout_path: &str,
        branch_names: &[String],
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Result<(), TrackError> {
        let script = CleanupReviewArtifactsScript;
        let arguments =
            script.arguments(checkout_path, branch_names, worktree_paths, run_directories);
        self.run_script(&script.render(), &arguments)?;

        Ok(())
    }

    fn cleanup_orphaned_remote_artifacts(
        &self,
        workspace_root: &str,
        kept_worktree_paths: &[String],
        kept_run_directories: &[String],
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        // The remote workspace layout is currently automation-owned:
        // `<workspace>/<name>/<name>` for the checkout plus sibling
        // task/review worktree and run directories. That lets one broad sweep
        // remove forgotten `dispatch-*` artifacts without needing a second
        // local registry of every worktree ever created.
        // TODO: If the checkout layout ever becomes user-configurable, replace
        // this directory derivation with a registry-backed lookup.
        let script = CleanupOrphanedRemoteArtifactsScript;
        let arguments = script.arguments(workspace_root, kept_worktree_paths, kept_run_directories);
        let report = self.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
    }

    fn cleanup_review_workspace_caches(&self, checkout_paths: &[String]) -> Result<(), TrackError> {
        if checkout_paths.is_empty() {
            return Ok(());
        }

        let script = CleanupReviewWorkspaceCachesScript;
        self.run_script(&script.render(), checkout_paths)?;

        Ok(())
    }

    fn reset_workspace(
        &self,
        workspace_root: &str,
        projects_registry_path: &str,
    ) -> Result<RemoteResetSummary, TrackError> {
        let script = ResetWorkspaceScript;
        let arguments = script.arguments(workspace_root, projects_registry_path);
        let report = self.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
    }

    fn read_remote_file(&self, remote_path: &str) -> Result<Option<String>, TrackError> {
        let script = ReadRemoteFileScript;
        let arguments = script.arguments(remote_path);
        match self.run_script_with_exit_code(&script.render(), &arguments)? {
            ScriptOutput::Success(stdout) => Ok(Some(stdout)),
            ScriptOutput::ExitCode(ReadRemoteFileScript::MISSING_FILE_EXIT_CODE) => Ok(None),
            ScriptOutput::ExitCode(code) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Could not read the remote file at {remote_path}: remote command exited with status code {code}."
                ),
            )),
            ScriptOutput::Failure(stderr) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not read the remote file at {remote_path}: {stderr}"),
            )),
        }
    }

    fn upload_remote_file(&self, remote_path: &str, contents: &str) -> Result<(), TrackError> {
        let script = PrepareRemoteUploadScript;
        let arguments = script.arguments(remote_path);
        self.run_script(&script.render(), &arguments)?;

        let local_temp_file = env::temp_dir().join(format!(
            "track-remote-upload-{}",
            now_utc().unix_timestamp_nanos()
        ));
        fs::write(&local_temp_file, contents).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not write a temporary upload file at {}: {error}",
                    path_to_string(&local_temp_file)
                ),
            )
        })?;

        let output = self
            .base_scp_command()
            .arg(&local_temp_file)
            .arg(format!("{}@{}:{remote_path}", self.user, self.host))
            .output()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Could not start `scp` for remote dispatch: {error}"),
                )
            })?;
        let _ = fs::remove_file(&local_temp_file);

        if !output.status.success() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Could not upload the remote file at {remote_path}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }

        Ok(())
    }

    fn read_dispatch_snapshots(
        &self,
        run_directories: &[String],
    ) -> Result<Vec<RemoteDispatchSnapshot>, TrackError> {
        if run_directories.is_empty() {
            return Ok(Vec::new());
        }

        let script = ReadDispatchSnapshotsScript;
        let report = self.run_script(&script.render(), run_directories)?;

        script.parse_report(&report)
    }

    fn run_script(&self, script: &str, args: &[String]) -> Result<String, TrackError> {
        match self.run_script_with_exit_code(script, args)? {
            ScriptOutput::Success(stdout) => Ok(stdout),
            ScriptOutput::ExitCode(code) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Remote command exited with unexpected status code {code}."),
            )),
            ScriptOutput::Failure(stderr) => {
                Err(TrackError::new(ErrorCode::RemoteDispatchFailed, stderr))
            }
        }
    }

    fn run_script_with_exit_code(
        &self,
        script: &str,
        args: &[String],
    ) -> Result<ScriptOutput, TrackError> {
        let mut command = self.base_ssh_command();
        command.arg(format!("{}@{}", self.user, self.host));
        command.arg("bash");
        command.arg("-s");
        command.arg("--");
        command.args(args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not start the remote SSH command: {error}"),
            )
        })?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not open stdin for the remote SSH command.",
            ));
        };
        let rendered_script = render_remote_script_with_shell_prelude(script, &self.shell_prelude);
        stdin
            .write_all(rendered_script.as_bytes())
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Could not write the remote shell script to SSH stdin: {error}"),
                )
            })?;
        drop(stdin);

        let output = child.wait_with_output().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not wait for the remote SSH command to finish: {error}"),
            )
        })?;

        if output.status.success() {
            return Ok(ScriptOutput::Success(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            ));
        }

        let exit_code = output.status.code();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if let Some(exit_code) = exit_code {
            if stderr.is_empty() {
                return Ok(ScriptOutput::ExitCode(exit_code));
            }

            if exit_code == 3 {
                return Ok(ScriptOutput::ExitCode(exit_code));
            }
        }

        Ok(ScriptOutput::Failure(if stderr.is_empty() {
            "Remote command failed without stderr output.".to_owned()
        } else {
            stderr
        }))
    }

    fn base_ssh_command(&self) -> Command {
        let mut command = Command::new("ssh");
        command.arg("-i");
        command.arg(&self.key_path);
        command.arg("-p");
        command.arg(self.port.to_string());
        command.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
        ]);
        command.arg(format!(
            "UserKnownHostsFile={}",
            path_to_string(&self.known_hosts_path)
        ));
        command
    }

    fn base_scp_command(&self) -> Command {
        let mut command = Command::new("scp");
        command.arg("-i");
        command.arg(&self.key_path);
        command.arg("-P");
        command.arg(self.port.to_string());
        command.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
        ]);
        command.arg(format!(
            "UserKnownHostsFile={}",
            path_to_string(&self.known_hosts_path)
        ));
        command
    }
}

enum ScriptOutput {
    Success(String),
    ExitCode(i32),
    Failure(String),
}

#[cfg(test)]
mod tests;
