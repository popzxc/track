use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, OnceLock};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::Duration;

use crate::backend_config::RemoteAgentConfigService;
use crate::dispatch_repository::DispatchRepository;
use crate::errors::{ErrorCode, TrackError};
use crate::paths::{collapse_home_path, path_to_string};
use crate::project_repository::{ProjectMetadata, ProjectRepository};
use crate::review_dispatch_repository::ReviewDispatchRepository;
use crate::review_repository::ReviewRepository;
use crate::task_description::{append_follow_up_request, parse_task_description};
use crate::task_id::build_unique_task_id;
use crate::task_repository::FileTaskRepository;
use crate::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_seconds};
use crate::types::{
    CreateReviewInput, DispatchStatus, RemoteAgentDispatchOutcome, RemoteAgentPreferredTool,
    RemoteAgentReviewOutcome, RemoteCleanupSummary, RemoteResetSummary, ReviewRecord,
    ReviewRunRecord, Status, TaskDispatchRecord, TaskUpdateInput,
};

const REMOTE_STATUS_FILE_NAME: &str = "status.txt";
const REMOTE_RESULT_FILE_NAME: &str = "result.json";
const REMOTE_STDERR_FILE_NAME: &str = "stderr.log";
const REMOTE_FINISHED_AT_FILE_NAME: &str = "finished-at.txt";
const REMOTE_PROMPT_FILE_NAME: &str = "prompt.md";
const REMOTE_SCHEMA_FILE_NAME: &str = "result-schema.json";
const REMOTE_LAUNCHER_PID_FILE_NAME: &str = "launcher.pid";
// We keep the historical sidecar filename for the child agent PID so users can
// still cancel or clean up runs that were launched before Claude support
// landed. The file now stores whichever remote agent process is active.
const REMOTE_CODEX_PID_FILE_NAME: &str = "codex.pid";
// Repository bootstrap can legitimately take a while on first clone or after a
// large fetch, so we keep the stale-preparing threshold generous. The API now
// also refreshes the summary at each preparation phase so normal progress keeps
// pushing this timeout forward instead of relying on one initial timestamp.
const PREPARING_STALE_AFTER: Duration = Duration::minutes(30);

const REVIEW_WORKTREE_DIRECTORY_NAME: &str = "review-worktrees";
const REVIEW_RUN_DIRECTORY_NAME: &str = "review-runs";

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RemoteDispatchSnapshot {
    status: Option<String>,
    result: Option<String>,
    stderr: Option<String>,
    finished_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RemoteArtifactCleanupCounts {
    worktrees_removed: usize,
    run_directories_removed: usize,
}

#[derive(Debug, Deserialize)]
struct RemoteArtifactCleanupReport {
    #[serde(rename = "worktreesRemoved")]
    worktrees_removed: usize,
    #[serde(rename = "runDirectoriesRemoved")]
    run_directories_removed: usize,
}

#[derive(Debug, Deserialize)]
struct RemoteWorkspaceResetReport {
    #[serde(rename = "workspaceEntriesRemoved")]
    workspace_entries_removed: usize,
    #[serde(rename = "registryRemoved")]
    registry_removed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteReviewFollowUpReconciliation {
    pub queued_dispatches: Vec<TaskDispatchRecord>,
    pub review_notifications_updated: usize,
    pub failures: usize,
    pub events: Vec<RemoteReviewFollowUpEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReviewFollowUpEvent {
    pub outcome: String,
    pub detail: String,
    pub task_id: String,
    pub dispatch_id: String,
    pub dispatch_status: String,
    pub remote_host: String,
    pub branch_name: Option<String>,
    pub pull_request_url: Option<String>,
    pub reviewer: String,
    pub pr_is_open: Option<bool>,
    pub pr_head_oid: Option<String>,
    pub latest_review_state: Option<String>,
    pub latest_review_submitted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPullRequestReference {
    owner: String,
    repository: String,
    number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPullRequestMetadata {
    pull_request_url: String,
    pull_request_number: u64,
    pull_request_title: String,
    repository_full_name: String,
    repo_url: String,
    git_url: String,
    base_branch: String,
    head_oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubPullRequestReviewState {
    is_open: bool,
    head_oid: String,
    latest_eligible_review: Option<GithubSubmittedReview>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubSubmittedReview {
    state: String,
    submitted_at: time::OffsetDateTime,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestApiResponse {
    state: String,
    title: String,
    #[serde(rename = "merged_at")]
    merged_at: Option<String>,
    base: GithubPullRequestBaseApiResponse,
    head: GithubPullRequestHeadApiResponse,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestBaseApiResponse {
    #[serde(rename = "ref")]
    branch_ref: String,
}

#[derive(Debug, Deserialize)]
struct GithubPullRequestHeadApiResponse {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GithubUserApiResponse {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubReviewApiResponse {
    state: String,
    #[serde(rename = "submitted_at")]
    submitted_at: Option<String>,
    user: Option<GithubUserApiResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RemoteProjectRegistryFile {
    version: u8,
    projects: BTreeMap<String, RemoteProjectRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RemoteProjectRegistryEntry {
    #[serde(rename = "checkoutPath")]
    checkout_path: String,
    #[serde(rename = "forkGitUrl")]
    fork_git_url: String,
    #[serde(rename = "repoUrl")]
    repo_url: String,
    #[serde(rename = "gitUrl")]
    git_url: String,
    #[serde(rename = "baseBranch")]
    base_branch: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

impl Default for RemoteProjectRegistryFile {
    fn default() -> Self {
        Self {
            version: 1,
            projects: BTreeMap::new(),
        }
    }
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
    pub fn update_task(
        &self,
        task_id: &str,
        input: TaskUpdateInput,
    ) -> Result<crate::types::Task, TrackError> {
        let validated_input = input.validate()?;

        if validated_input.status == Some(crate::types::Status::Closed) {
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
    ) -> Result<crate::types::Task, TrackError> {
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
    ) -> Result<crate::types::RemoteAgentRuntimeConfig, TrackError> {
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

    fn load_remote_agent_for_global_cleanup(
        &self,
    ) -> Result<crate::types::RemoteAgentRuntimeConfig, TrackError> {
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
        remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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
        remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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
    ) -> Result<
        (
            crate::types::RemoteAgentRuntimeConfig,
            crate::types::Task,
            ProjectMetadata,
        ),
        TrackError,
    > {
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
        remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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
    ) -> Result<crate::types::RemoteAgentRuntimeConfig, TrackError> {
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
        remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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
    fn load_review_runner_prerequisites(
        &self,
    ) -> Result<crate::types::RemoteAgentRuntimeConfig, TrackError> {
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
            crate::types::RemoteAgentRuntimeConfig,
            crate::types::RemoteAgentReviewFollowUpRuntimeConfig,
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
    ) -> Result<(crate::types::RemoteAgentRuntimeConfig, ReviewRecord), TrackError> {
        let remote_agent = self.load_review_runner_prerequisites()?;
        let review = self.review_repository.get_review(review_id)?;

        Ok((remote_agent, review))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteTaskCleanupMode {
    CloseTask,
    DeleteTask,
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
    remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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
    remote_agent: &crate::types::RemoteAgentRuntimeConfig,
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

fn derive_review_run_directory_for_record(
    record: &ReviewRunRecord,
    remote_agent: &crate::types::RemoteAgentRuntimeConfig,
) -> Option<String> {
    if let Some(worktree_path) = record.worktree_path.as_deref() {
        if let Ok(run_directory) = derive_review_run_directory(worktree_path, &record.dispatch_id) {
            return Some(run_directory);
        }
    }

    if record.workspace_key.trim().is_empty() || remote_agent.workspace_root.trim().is_empty() {
        return None;
    }

    Some(format!(
        "{}/{}/{}/{}",
        remote_agent.workspace_root.trim_end_matches('/'),
        record.workspace_key,
        REVIEW_RUN_DIRECTORY_NAME,
        record.dispatch_id
    ))
}

fn build_create_review_worktree_script() -> String {
    format!(
        r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
PULL_REQUEST_NUMBER="$2"
BRANCH_NAME="$3"
WORKTREE_PATH="$(expand_remote_path "$4")"
TARGET_HEAD_OID="${{5:-}}"

mkdir -p "$(dirname "$WORKTREE_PATH")"

worktree_is_registered() {{
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $WORKTREE_PATH" >/dev/null 2>&1
}}

if [ -e "$WORKTREE_PATH" ]; then
  if worktree_is_registered; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  else
    echo "Refusing to overwrite unexpected existing path at $WORKTREE_PATH while preparing a review worktree." >&2
    exit 1
  fi
fi

git -C "$CHECKOUT_PATH" worktree prune >&2
git -C "$CHECKOUT_PATH" fetch upstream "pull/$PULL_REQUEST_NUMBER/head:$BRANCH_NAME" >&2

# Review runs persist the exact PR head they were queued against. We still
# refresh the PR ref so the checkout has current GitHub context, but then we
# pin the local review branch back to the recorded commit when that object is
# available. If the commit is gone, we fail explicitly instead of silently
# reviewing a newer PR head than the user requested.
TARGET_REF="$BRANCH_NAME"
if [ -n "$TARGET_HEAD_OID" ]; then
  if ! git -C "$CHECKOUT_PATH" cat-file -e "$TARGET_HEAD_OID^{{commit}}" 2>/dev/null; then
    git -C "$CHECKOUT_PATH" fetch upstream "$TARGET_HEAD_OID" >&2 || true
  fi

  if git -C "$CHECKOUT_PATH" cat-file -e "$TARGET_HEAD_OID^{{commit}}" 2>/dev/null; then
    TARGET_REF="$TARGET_HEAD_OID"
  else
    FETCHED_HEAD_OID="$(git -C "$CHECKOUT_PATH" rev-parse "$BRANCH_NAME^{{commit}}")"
    echo "Requested review commit $TARGET_HEAD_OID is not available locally. The fetched PR head is $FETCHED_HEAD_OID, so the review would drift to a newer commit." >&2
    exit 1
  fi
fi

git -C "$CHECKOUT_PATH" branch -f "$BRANCH_NAME" "$TARGET_REF" >&2
git -C "$CHECKOUT_PATH" worktree add -B "$BRANCH_NAME" "$WORKTREE_PATH" "$TARGET_REF" >&2
"#,
        path_helpers = remote_path_helpers_shell(),
    )
}

fn unique_review_worktree_paths(dispatch_history: &[ReviewRunRecord]) -> Vec<String> {
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

fn unique_review_run_directories(
    dispatch_history: &[ReviewRunRecord],
    remote_agent: &crate::types::RemoteAgentRuntimeConfig,
) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| derive_review_run_directory_for_record(record, remote_agent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn describe_remote_reset_blockers(
    task_dispatches: &[TaskDispatchRecord],
    review_dispatches: &[ReviewRunRecord],
) -> Vec<String> {
    let mut blockers = task_dispatches
        .iter()
        .filter(|record| record.status.is_active())
        .map(|record| format!("task {} ({})", record.task_id, record.dispatch_id))
        .collect::<Vec<_>>();
    blockers.extend(
        review_dispatches
            .iter()
            .filter(|record| record.status.is_active())
            .map(|record| format!("review {} ({})", record.review_id, record.dispatch_id)),
    );
    blockers
}

fn build_remote_dispatch_prompt(
    project_name: &str,
    metadata: &ProjectMetadata,
    branch_name: &str,
    worktree_path: &str,
    task_description: &str,
    pull_request_url: Option<&str>,
    follow_up_request: Option<&str>,
) -> String {
    let sections = parse_task_description(task_description);
    let mut prompt = String::new();
    prompt.push_str("# Remote task dispatch\n\n");
    prompt.push_str(
        "You are working in a fully autonomous mode on a prepared repository worktree.\n",
    );
    prompt.push_str("The repository checkout, fork, and worktree are already set up for you.\n");
    prompt.push_str("You have full filesystem access, internet access, and `gh` is available.\n");
    prompt.push_str("Make the decisions needed to complete the task responsibly.\n");
    prompt.push_str(
        "The desired outcome is a GitHub PR unless the task is blocked or cannot be solved.\n\n",
    );
    prompt.push_str("## Repository context\n\n");
    prompt.push_str(&format!("- Project: {project_name}\n"));
    prompt.push_str(&format!("- Repo URL: {}\n", metadata.repo_url));
    prompt.push_str(&format!("- Git URL: {}\n", metadata.git_url));
    prompt.push_str(&format!("- Base branch: {}\n", metadata.base_branch));
    prompt.push_str(&format!("- Prepared branch: {branch_name}\n"));
    prompt.push_str(&format!("- Working directory: {worktree_path}\n\n"));

    if let Some(pull_request_url) = pull_request_url.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("## Existing PR\n\n");
        prompt.push_str(&format!("- Pull request: {pull_request_url}\n"));
        prompt.push_str(
            "- Continue working on this existing PR with the same prepared branch and worktree.\n",
        );
        prompt.push_str(
            "- Do not open a second PR unless the current PR is unusable and you explain why.\n\n",
        );
    }

    prompt.push_str("## Expectations\n\n");
    prompt.push_str("- Pull the task through to a GitHub PR when possible.\n");
    prompt.push_str("- Use the current worktree as the only place to make changes.\n");
    prompt.push_str("- Use conventional commits for both commit messages and the PR title, for example `feat: Add X`, `fix: Correct Y`, or `chore: Update Z`.\n");
    prompt.push_str("- If the follow-up mentions review comments or reviewer feedback, fetch that context with `gh` instead of guessing.\n");
    prompt.push_str("- If the follow-up names a reviewer, only act on that reviewer's feedback unless the request explicitly says otherwise.\n");
    prompt.push_str("- If the task is blocked, explain the blocker clearly in the final JSON.\n\n");
    prompt.push_str("## Task title\n\n");
    prompt.push_str(&sections.title);
    prompt.push_str("\n\n");

    if let Some(summary_markdown) = sections.summary_markdown.as_deref() {
        prompt.push_str("## Summary\n\n");
        prompt.push_str(summary_markdown);
        prompt.push_str("\n\n");
    }

    if let Some(original_note) = sections.original_note.as_deref() {
        prompt.push_str("## Original note\n\n");
        prompt.push_str(original_note);
        prompt.push_str("\n\n");
    }

    if let Some(follow_up_request) = follow_up_request.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("## Current follow-up request\n\n");
        prompt.push_str(follow_up_request.trim());
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Final response\n\n");
    prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

    prompt
}

fn build_remote_review_prompt(
    review: &ReviewRecord,
    dispatch_record: &ReviewRunRecord,
    previous_submitted_review: Option<&ReviewRunRecord>,
) -> String {
    let branch_name = dispatch_record
        .branch_name
        .as_deref()
        .expect("queued review dispatches should always have a branch name");
    let worktree_path = dispatch_record
        .worktree_path
        .as_deref()
        .expect("queued review dispatches should always have a worktree path");
    let mut prompt = String::new();
    prompt.push_str("# Remote PR review\n\n");
    prompt.push_str(
        "You are reviewing an existing GitHub pull request from a prepared repository worktree.\n",
    );
    prompt.push_str("The repository checkout and review worktree are already prepared for you.\n");
    prompt.push_str("You have full filesystem access, internet access, and `gh` is available.\n");
    prompt.push_str("This run is for review only: do not push commits, open PRs, or request reviewers yourself.\n");
    prompt.push_str("You are responsible for submitting the GitHub review yourself before you return the final JSON.\n\n");
    prompt.push_str("## Pull request context\n\n");
    prompt.push_str(&format!("- Pull request: {}\n", review.pull_request_url));
    prompt.push_str(&format!("- Title: {}\n", review.pull_request_title));
    prompt.push_str(&format!("- Repository: {}\n", review.repository_full_name));
    prompt.push_str(&format!("- Repo URL: {}\n", review.repo_url));
    prompt.push_str(&format!("- Base branch: {}\n", review.base_branch));
    prompt.push_str(&format!("- Prepared branch: {branch_name}\n"));
    prompt.push_str(&format!("- Working directory: {worktree_path}\n"));
    if let Some(target_head_oid) = dispatch_record.target_head_oid.as_deref() {
        prompt.push_str(&format!("- Pinned review commit: {target_head_oid}\n"));
    }
    prompt.push('\n');
    prompt.push_str("## Review instructions\n\n");
    prompt.push_str("- Submit one GitHub review in COMMENT mode.\n");
    prompt.push_str(&format!(
        "- The first line of the top-level review body must be `@{} requested me to review this PR.`\n",
        review.main_user
    ));
    prompt.push_str("- Prefer inline review comments for concrete file/line findings so people can reply in GitHub threads.\n");
    prompt.push_str("- Use the top-level review body for the overall summary, major risks, and any no-findings conclusion.\n");
    prompt.push_str(
        "- Focus on bugs, regressions, risky behavior changes, missing tests, and edge cases.\n",
    );
    prompt.push_str("- Use the checked-out code and `gh` to inspect the PR diff and context instead of guessing.\n");
    prompt.push_str("- If a pinned review commit is listed above, the prepared worktree is intended to match that exact commit. If it does not, stop and explain the mismatch instead of reviewing a newer head silently.\n");
    prompt.push_str("- Keep the review concise but concrete.\n");
    prompt.push_str(
        "- If you do not find problems, say so explicitly in the top-level review body.\n",
    );
    prompt.push_str("- If you cannot complete the review responsibly, explain the blocker in the summary and do not claim the review was submitted.\n");
    prompt.push_str("- Capture the submitted GitHub review's durable handle from the `gh` response and return it as `githubReviewId` and `githubReviewUrl` when submission succeeds.\n");
    prompt.push_str("- Return `reviewSubmitted` as `true` only after GitHub confirms that the review submission succeeded.\n\n");

    if let Some(follow_up_request) = dispatch_record.follow_up_request.as_deref() {
        prompt.push_str("## Current re-review request\n\n");
        prompt.push_str(follow_up_request.trim());
        prompt.push_str("\n\n");
    }

    if let Some(previous_submitted_review) = previous_submitted_review {
        prompt.push_str("## Previous bot review context\n\n");
        if let Some(github_review_url) = previous_submitted_review.github_review_url.as_deref() {
            prompt.push_str(&format!(
                "- Previous submitted review: {github_review_url}\n"
            ));
        }
        if let Some(github_review_id) = previous_submitted_review.github_review_id.as_deref() {
            prompt.push_str(&format!(
                "- Previous submitted review id: {github_review_id}\n"
            ));
        }
        if let Some(target_head_oid) = previous_submitted_review.target_head_oid.as_deref() {
            prompt.push_str(&format!(
                "- Previous review pinned commit: {target_head_oid}\n"
            ));
        }
        prompt.push('\n');
        prompt.push_str("## Re-review guidance\n\n");
        prompt.push_str("- Inspect the current PR conversation on GitHub before deciding whether an older bot finding still matters.\n");
        prompt.push_str(&format!(
            "- For context: your previous comments are always non-blocking input at the discretion of the reviewee unless @{} explicitly commented that a finding is valid and should be fixed.\n",
            review.main_user
        ));
        prompt.push_str(&format!(
            "- Only treat an older bot finding as something you must actively verify and potentially elevate into a primary finding if @{} explicitly said it is valid and should be fixed.\n",
            review.main_user
        ));
        prompt.push_str(&format!(
            "- If @{} or the reviewee explicitly said an older bot finding is not important, disputed it, or chose not to address it, do not repeat it as a primary finding just because it appeared in a previous bot review.\n",
            review.main_user
        ));
        prompt.push_str("- You may mention unresolved prior bot comments as brief context in the top-level summary when helpful, but re-evaluate the current code on its own merits.\n\n");
    }

    if let Some(default_review_prompt) = review.default_review_prompt.as_deref() {
        prompt.push_str("## Default review prompt\n\n");
        prompt.push_str(default_review_prompt);
        prompt.push_str("\n\n");
    }

    if let Some(extra_instructions) = review.extra_instructions.as_deref() {
        prompt.push_str("## Extra instructions\n\n");
        prompt.push_str(extra_instructions);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Final response\n\n");
    prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

    prompt
}

fn build_remote_dispatch_schema() -> String {
    serde_json::to_string_pretty(&json!({
        "type": "object",
        "additionalProperties": false,
        // Codex's structured-output validator is stricter than generic JSON
        // Schema consumers here: every declared property must appear in the
        // top-level `required` array, and optionality is expressed with
        // `null` in the property's type instead of omitting the field.
        "required": [
            "status",
            "summary",
            "pullRequestUrl",
            "branchName",
            "worktreePath",
            "notes"
        ],
        "properties": {
            "status": {
                "type": "string",
                "enum": ["succeeded", "failed", "blocked"]
            },
            "summary": {
                "type": "string"
            },
            "pullRequestUrl": {
                "type": ["string", "null"]
            },
            "branchName": {
                "type": ["string", "null"]
            },
            "worktreePath": {
                "type": "string"
            },
            "notes": {
                "type": ["string", "null"]
            }
        }
    }))
    .expect("dispatch schema serialization should succeed")
}

fn build_remote_review_schema() -> String {
    serde_json::to_string_pretty(&json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "status",
            "summary",
            "reviewSubmitted",
            "githubReviewId",
            "githubReviewUrl",
            "worktreePath",
            "notes"
        ],
        "properties": {
            "status": {
                "type": "string",
                "enum": ["succeeded", "failed", "blocked"]
            },
            "summary": {
                "type": "string"
            },
            "reviewSubmitted": {
                "type": "boolean"
            },
            "githubReviewId": {
                "type": ["string", "null"]
            },
            "githubReviewUrl": {
                "type": ["string", "null"]
            },
            "worktreePath": {
                "type": "string"
            },
            "notes": {
                "type": ["string", "null"]
            }
        }
    }))
    .expect("review schema serialization should succeed")
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

fn parse_github_repository_name(repo_url: &str) -> Result<String, TrackError> {
    let trimmed = repo_url.trim().trim_end_matches('/');
    let without_suffix = trimmed.trim_end_matches(".git");
    let Some(repository_name) = without_suffix.rsplit('/').next() else {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Repo URL {repo_url} does not look like a GitHub repository."),
        ));
    };

    if !without_suffix.contains("github.com/") || repository_name.is_empty() {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Repo URL {repo_url} does not look like a GitHub repository."),
        ));
    }

    Ok(repository_name.to_owned())
}

fn parse_github_pull_request_reference(
    pull_request_url: &str,
) -> Result<GithubPullRequestReference, TrackError> {
    let trimmed = pull_request_url.trim().trim_end_matches('/');
    let without_scheme = trimmed.strip_prefix("https://github.com/").ok_or_else(|| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!(
                "Pull request URL {pull_request_url} does not look like a GitHub pull request."
            ),
        )
    })?;
    let parts = without_scheme.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!(
                "Pull request URL {pull_request_url} does not look like a GitHub pull request."
            ),
        ));
    }

    let number = parts[3].parse::<u64>().map_err(|_| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Pull request URL {pull_request_url} does not contain a valid PR number."),
        )
    })?;

    Ok(GithubPullRequestReference {
        owner: parts[0].to_owned(),
        repository: parts[1].to_owned(),
        number,
    })
}

fn build_review_workspace_key(pull_request: &GithubPullRequestMetadata) -> String {
    let slug = slug::slugify(
        pull_request
            .repository_full_name
            .replace('/', "-")
            .trim(),
    );

    if slug.is_empty() {
        "review-repo".to_owned()
    } else {
        slug
    }
}

fn build_review_follow_up_request(
    pull_request_url: &str,
    main_user: &str,
    dispatch_started_at: time::OffsetDateTime,
) -> String {
    format!(
        "Respond to new review feedback from @{main_user} on the existing PR.\n\n\
Use `gh` to fetch submitted PR reviews and inline review comments from @{main_user} only.\n\
Only use reviews with state COMMENTED or CHANGES_REQUESTED that were submitted after {dispatch_started_at}.\n\
Ignore APPROVED reviews and all feedback from other users.\n\
Keep using the existing PR at {pull_request_url} unless you explain why that is impossible.",
        dispatch_started_at = format_iso_8601_millis(dispatch_started_at),
    )
}

fn build_review_follow_up_notification_comment(main_user: &str, head_oid: &str) -> String {
    let short_head_oid = head_oid.get(..7).unwrap_or(head_oid);

    format!(
        "@{main_user} new bot updates are ready on commit `{short_head_oid}`. \
Please leave a PR review (COMMENTED or CHANGES_REQUESTED) if you want the bot to follow up automatically."
    )
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

fn contextualize_track_error(error: TrackError, context: impl Into<String>) -> TrackError {
    TrackError::new(
        error.code,
        format!("{}: {}", context.into(), error.message()),
    )
}

fn review_follow_up_event(
    outcome: &str,
    detail: impl Into<String>,
    dispatch_record: &TaskDispatchRecord,
    reviewer: &str,
    pull_request_state: Option<&GithubPullRequestReviewState>,
) -> RemoteReviewFollowUpEvent {
    let latest_review_state = pull_request_state
        .and_then(|state| state.latest_eligible_review.as_ref())
        .map(|review| review.state.clone());
    let latest_review_submitted_at = pull_request_state
        .and_then(|state| state.latest_eligible_review.as_ref())
        .map(|review| format_iso_8601_millis(review.submitted_at));

    RemoteReviewFollowUpEvent {
        outcome: outcome.to_owned(),
        detail: detail.into(),
        task_id: dispatch_record.task_id.clone(),
        dispatch_id: dispatch_record.dispatch_id.clone(),
        dispatch_status: dispatch_record.status.as_str().to_owned(),
        remote_host: dispatch_record.remote_host.clone(),
        branch_name: dispatch_record.branch_name.clone(),
        pull_request_url: dispatch_record.pull_request_url.clone(),
        reviewer: reviewer.to_owned(),
        pr_is_open: pull_request_state.map(|state| state.is_open),
        pr_head_oid: pull_request_state.map(|state| state.head_oid.clone()),
        latest_review_state,
        latest_review_submitted_at,
    }
}

fn remote_path_helpers_shell() -> &'static str {
    r#"
expand_remote_path() {
  case "$1" in
    "~")
      printf '%s\n' "$HOME"
      ;;
    "~/"*)
      printf '%s/%s\n' "$HOME" "${1#~/}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}
"#
}

fn render_remote_script_with_shell_prelude(script: &str, shell_prelude: &str) -> String {
    let mut rendered = String::from("set -e\n");

    // The runner prelude is intentionally user-managed shell code. We execute
    // it before each remote command so PATH setup and toolchain activation stay
    // explicit instead of depending on interactive shell startup files that
    // SSH automation does not reliably inherit.
    if !shell_prelude.trim().is_empty() {
        rendered.push_str(shell_prelude);
        if !shell_prelude.ends_with('\n') {
            rendered.push('\n');
        }
    }

    rendered.push('\n');
    rendered.push_str(script.trim_start_matches('\n'));
    rendered
}

fn build_remote_agent_command(preferred_tool: RemoteAgentPreferredTool) -> String {
    match preferred_tool {
        RemoteAgentPreferredTool::Codex => format!(
            "codex --search exec --dangerously-bypass-approvals-and-sandbox -C \"$WORKTREE_PATH\" --json --output-schema \"$RUN_DIR/{REMOTE_SCHEMA_FILE_NAME}\" -o \"$RUN_DIR/{REMOTE_RESULT_FILE_NAME}\" - < \"$RUN_DIR/{REMOTE_PROMPT_FILE_NAME}\" > \"$RUN_DIR/events.jsonl\" 2> \"$RUN_DIR/{REMOTE_STDERR_FILE_NAME}\" &\n"
        ),
        RemoteAgentPreferredTool::Claude => {
            let mut command = String::new();
            command.push_str(&format!(
                "SCHEMA_CONTENT=\"$(tr -d '\\n' < \"$RUN_DIR/{REMOTE_SCHEMA_FILE_NAME}\")\"\n"
            ));
            command.push_str("cd \"$WORKTREE_PATH\"\n");
            // Claude Code's JSON-schema mode emits request metadata plus the
            // validated payload under `structured_output`, so we launch it in
            // JSON mode and let the refresh path unwrap that envelope later.
            command.push_str(&format!(
                "claude -p --dangerously-skip-permissions --add-dir \"$WORKTREE_PATH\" --output-format json --json-schema \"$SCHEMA_CONTENT\" < \"$RUN_DIR/{REMOTE_PROMPT_FILE_NAME}\" > \"$RUN_DIR/{REMOTE_RESULT_FILE_NAME}\" 2> \"$RUN_DIR/{REMOTE_STDERR_FILE_NAME}\" &\n"
            ));
            command
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeStructuredOutputEnvelope<T> {
    #[serde(rename = "structured_output")]
    structured_output: T,
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

fn build_remote_agent_launcher(
    preferred_tool: RemoteAgentPreferredTool,
    shell_prelude: &str,
) -> String {
    let mut launcher = String::from("#!/usr/bin/env bash\n");
    launcher.push_str("set -e\n");
    if !shell_prelude.trim().is_empty() {
        launcher.push_str(shell_prelude);
        if !shell_prelude.ends_with('\n') {
            launcher.push('\n');
        }
    }

    launcher.push_str("set -eu\n");
    launcher.push_str("RUN_DIR=\"$1\"\n");
    launcher.push_str("WORKTREE_PATH=\"$2\"\n");
    launcher.push_str(&format!(
        "printf '%s\\n' \"$$\" > \"$RUN_DIR/{REMOTE_LAUNCHER_PID_FILE_NAME}\"\n"
    ));
    launcher.push_str("cancel_run() {\n");
    launcher.push_str(&format!(
        "  if [ -f \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\" ]; then\n"
    ));
    launcher.push_str(&format!(
        "    CODEX_PID=\"$(tr -d '[:space:]' < \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\")\"\n"
    ));
    launcher.push_str("    if [ -n \"$CODEX_PID\" ] && kill -0 \"$CODEX_PID\" 2>/dev/null; then\n");
    launcher.push_str("      kill \"$CODEX_PID\" 2>/dev/null || true\n");
    launcher.push_str("    fi\n");
    launcher.push_str("  fi\n");
    launcher.push_str(&format!(
        "  printf 'canceled\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
    ));
    launcher.push_str(&format!(
        "  date -u +%Y-%m-%dT%H:%M:%SZ > \"$RUN_DIR/{REMOTE_FINISHED_AT_FILE_NAME}\"\n"
    ));
    launcher.push_str("  exit 130\n");
    launcher.push_str("}\n");
    launcher.push_str("trap cancel_run TERM INT\n");
    launcher.push_str(&format!(
        "printf 'running\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
    ));
    launcher.push_str(&build_remote_agent_command(preferred_tool));
    launcher.push_str("CODEX_PID=\"$!\"\n");
    launcher.push_str(&format!(
        "printf '%s\\n' \"$CODEX_PID\" > \"$RUN_DIR/{REMOTE_CODEX_PID_FILE_NAME}\"\n"
    ));
    launcher.push_str("if wait \"$CODEX_PID\"; then\n");
    launcher.push_str(&format!(
        "  printf 'completed\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
    ));
    launcher.push_str("else\n");
    launcher.push_str("  EXIT_CODE=\"$?\"\n");
    launcher.push_str(&format!(
        "  CURRENT_STATUS=\"$(tr -d '[:space:]' < \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\" 2>/dev/null || true)\"\n"
    ));
    launcher.push_str(
        "  if [ \"$CURRENT_STATUS\" != \"canceled\" ] && [ \"$EXIT_CODE\" -ne 130 ]; then\n",
    );
    launcher.push_str(&format!(
        "    printf 'launcher_failed\\n' > \"$RUN_DIR/{REMOTE_STATUS_FILE_NAME}\"\n"
    ));
    launcher.push_str("  fi\n");
    launcher.push_str("fi\n");
    launcher.push_str(&format!(
        "date -u +%Y-%m-%dT%H:%M:%SZ > \"$RUN_DIR/{REMOTE_FINISHED_AT_FILE_NAME}\"\n"
    ));
    launcher
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
    fn new(config: &crate::types::RemoteAgentRuntimeConfig) -> Result<Self, TrackError> {
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
        let login = self.run_script(
            r#"
set -eu
gh api user --jq .login
"#,
            &[],
        )?;

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
        let pull_request_json = self
            .run_script(
                r#"
set -eu
ENDPOINT="$1"
gh api "$ENDPOINT"
"#,
                std::slice::from_ref(&pull_request_endpoint),
            )
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
        let pull_request_json = self
            .run_script(
                r#"
set -eu
ENDPOINT="$1"
gh api "$ENDPOINT"
"#,
                std::slice::from_ref(&pull_request_endpoint),
            )
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
        let reviews_json = self
            .run_script(
                r#"
set -eu
ENDPOINT="$1"
gh api "$ENDPOINT"
"#,
                std::slice::from_ref(&reviews_endpoint),
            )
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
        self.run_script(
            r#"
set -eu
ENDPOINT="$1"
BODY="$2"
gh api --method POST "$ENDPOINT" -f body="$BODY" >/dev/null
"#,
            &[issue_comments_endpoint.clone(), comment_body.to_owned()],
        )
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
        let ensure_checkout_script = format!(
            r#"
set -eu
{path_helpers}
REPO_URL="$1"
REPOSITORY_NAME="$2"
GIT_URL="$3"
BASE_BRANCH="$4"
CHECKOUT_PATH="$(expand_remote_path "$5")"
GITHUB_LOGIN="$6"

mkdir -p "$(dirname "$CHECKOUT_PATH")"

# Remote automation runs on fresh machines too, so Git cannot assume that
# GitHub already exists in the remote user's known_hosts file. We explicitly
# manage a predictable known_hosts path here and tell Git to accept the first
# key it sees. That keeps the initial clone/fetch flow unattended while still
# recording the host key for the next command.
REMOTE_SSH_DIR="$HOME/.ssh"
REMOTE_KNOWN_HOSTS_PATH="$REMOTE_SSH_DIR/known_hosts"
mkdir -p "$REMOTE_SSH_DIR"
chmod 700 "$REMOTE_SSH_DIR"
touch "$REMOTE_KNOWN_HOSTS_PATH"
chmod 600 "$REMOTE_KNOWN_HOSTS_PATH"
export GIT_SSH_COMMAND="ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=$REMOTE_KNOWN_HOSTS_PATH"

resolve_fork_git_url() {{
  gh repo view "$GITHUB_LOGIN/$REPOSITORY_NAME" --json sshUrl --jq .sshUrl 2>/dev/null || true
}}

FORK_GIT_URL="$(resolve_fork_git_url)"
if [ -z "$FORK_GIT_URL" ]; then
  gh repo fork "$REPO_URL" >/dev/null
  FORK_GIT_URL="$(resolve_fork_git_url)"
fi

if [ -z "$FORK_GIT_URL" ]; then
  echo "Could not determine the fork SSH URL for $GITHUB_LOGIN/$REPOSITORY_NAME after creating the fork." >&2
  exit 1
fi

if [ ! -d "$CHECKOUT_PATH/.git" ]; then
  git clone "$FORK_GIT_URL" "$CHECKOUT_PATH" >&2
fi

cd "$CHECKOUT_PATH"
if git remote get-url origin >/dev/null 2>&1; then
  git remote set-url origin "$FORK_GIT_URL"
else
  git remote add origin "$FORK_GIT_URL"
fi

if git remote get-url upstream >/dev/null 2>&1; then
  git remote set-url upstream "$GIT_URL"
else
  git remote add upstream "$GIT_URL"
fi

git fetch origin --prune >&2
git fetch upstream --prune >&2

if git show-ref --verify --quiet "refs/heads/$BASE_BRANCH"; then
  git checkout "$BASE_BRANCH" >&2
else
  git checkout -B "$BASE_BRANCH" "upstream/$BASE_BRANCH" >&2
fi

git reset --hard "upstream/$BASE_BRANCH" >&2
git clean -fd >&2

printf '%s\n' "$FORK_GIT_URL"
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        let fork_git_url = self.run_script(
            &ensure_checkout_script,
            &[
                metadata.repo_url.clone(),
                repository_name.to_owned(),
                metadata.git_url.clone(),
                metadata.base_branch.clone(),
                checkout_path.to_owned(),
                github_login.to_owned(),
            ],
        )?;

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
        let create_worktree_script = format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
BASE_BRANCH="$2"
BRANCH_NAME="$3"
WORKTREE_PATH="$(expand_remote_path "$4")"

mkdir -p "$(dirname "$WORKTREE_PATH")"

worktree_is_registered() {{
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $WORKTREE_PATH" >/dev/null 2>&1
}}

if [ -e "$WORKTREE_PATH" ]; then
  if worktree_is_registered; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  else
    echo "Refusing to overwrite unexpected existing path at $WORKTREE_PATH while preparing a fresh dispatch worktree." >&2
    exit 1
  fi
fi

git -C "$CHECKOUT_PATH" worktree prune >&2
git -C "$CHECKOUT_PATH" worktree add -B "$BRANCH_NAME" "$WORKTREE_PATH" "upstream/$BASE_BRANCH" >&2
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(
            &create_worktree_script,
            &[
                checkout_path.to_owned(),
                base_branch.to_owned(),
                branch_name.to_owned(),
                worktree_path.to_owned(),
            ],
        )?;

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
        let create_review_worktree_script = build_create_review_worktree_script();
        self.run_script(
            &create_review_worktree_script,
            &[
                checkout_path.to_owned(),
                pull_request_number.to_string(),
                branch_name.to_owned(),
                worktree_path.to_owned(),
                target_head_oid.unwrap_or_default().to_owned(),
            ],
        )?;

        Ok(())
    }

    fn ensure_follow_up_worktree(
        &self,
        checkout_path: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Result<(), TrackError> {
        let ensure_follow_up_worktree_script = format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
BRANCH_NAME="$2"
WORKTREE_PATH="$(expand_remote_path "$3")"

mkdir -p "$(dirname "$WORKTREE_PATH")"
git -C "$CHECKOUT_PATH" fetch origin --prune >&2 || true
git -C "$CHECKOUT_PATH" fetch upstream --prune >&2 || true

if [ -e "$WORKTREE_PATH/.git" ]; then
  if ! git -C "$WORKTREE_PATH" rev-parse --show-toplevel >/dev/null 2>&1; then
    echo "Existing follow-up worktree path $WORKTREE_PATH is not a valid Git worktree." >&2
    exit 1
  fi

  git -C "$WORKTREE_PATH" checkout "$BRANCH_NAME" >&2
  exit 0
fi

if [ -e "$WORKTREE_PATH" ]; then
  echo "Follow-up worktree path $WORKTREE_PATH already exists but is not a Git worktree." >&2
  exit 1
fi

git -C "$CHECKOUT_PATH" worktree prune >&2

if git -C "$CHECKOUT_PATH" show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
  git -C "$CHECKOUT_PATH" worktree add "$WORKTREE_PATH" "$BRANCH_NAME" >&2
  exit 0
fi

if git -C "$CHECKOUT_PATH" show-ref --verify --quiet "refs/remotes/origin/$BRANCH_NAME"; then
  git -C "$CHECKOUT_PATH" worktree add -B "$BRANCH_NAME" "$WORKTREE_PATH" "origin/$BRANCH_NAME" >&2
  exit 0
fi

echo "Could not restore the follow-up worktree for branch $BRANCH_NAME." >&2
exit 1
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(
            &ensure_follow_up_worktree_script,
            &[
                checkout_path.to_owned(),
                branch_name.to_owned(),
                worktree_path.to_owned(),
            ],
        )?;

        Ok(())
    }

    fn launch_remote_dispatch(
        &self,
        remote_run_directory: &str,
        worktree_path: &str,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Result<(), TrackError> {
        let launcher_contents = build_remote_agent_launcher(preferred_tool, &self.shell_prelude);
        self.upload_remote_file(
            &format!("{remote_run_directory}/launch.sh"),
            &launcher_contents,
        )?;

        let launch_script = format!(
            r#"
set -eu
{path_helpers}
RUN_DIR="$(expand_remote_path "$1")"
WORKTREE_PATH="$(expand_remote_path "$2")"

mkdir -p "$RUN_DIR"
LAUNCHER_PATH="$RUN_DIR/launch.sh"
chmod +x "$LAUNCHER_PATH"
nohup bash "$LAUNCHER_PATH" "$RUN_DIR" "$WORKTREE_PATH" >/dev/null 2>&1 </dev/null &
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(
            &launch_script,
            &[remote_run_directory.to_owned(), worktree_path.to_owned()],
        )?;

        Ok(())
    }

    fn cancel_remote_dispatch(&self, remote_run_directory: &str) -> Result<(), TrackError> {
        let cancel_script = format!(
            r#"
set -eu
{path_helpers}
RUN_DIR="$(expand_remote_path "$1")"
LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"
STATUS_FILE="$RUN_DIR/{status_file}"
FINISHED_AT_FILE="$RUN_DIR/{finished_at_file}"

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

if [ -f "$LAUNCHER_PID_FILE" ]; then
  LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
  kill_if_running "$LAUNCHER_PID"
fi

if [ -f "$CODEX_PID_FILE" ]; then
  CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
  kill_if_running "$CODEX_PID"
fi

mkdir -p "$RUN_DIR"
printf 'canceled\n' > "$STATUS_FILE"
date -u +%Y-%m-%dT%H:%M:%SZ > "$FINISHED_AT_FILE"
"#,
            path_helpers = remote_path_helpers_shell(),
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
            status_file = REMOTE_STATUS_FILE_NAME,
            finished_at_file = REMOTE_FINISHED_AT_FILE_NAME,
        );
        self.run_script(&cancel_script, &[remote_run_directory.to_owned()])?;
        Ok(())
    }

    fn cleanup_task_artifacts(
        &self,
        checkout_path: &str,
        worktree_paths: &[String],
        run_directories: &[String],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let cleanup_remote_dispatch_directories = cleanup_mode == RemoteTaskCleanupMode::DeleteTask;
        let cleanup_script = format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
shift

WORKTREE_PATHS=()
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--" ]; then
    shift
    break
  fi

  WORKTREE_PATHS+=("$1")
  shift
done

RUN_DIRECTORIES=("$@")
WORKTREES_REMOVED=0
RUN_DIRECTORIES_REMOVED=0

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

worktree_is_registered() {{
  TARGET_WORKTREE="$1"
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $TARGET_WORKTREE" >/dev/null 2>&1
}}

for RAW_RUN_DIR in "${{RUN_DIRECTORIES[@]}}"; do
  RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
  LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
  CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"
  STATUS_FILE="$RUN_DIR/{status_file}"
  FINISHED_AT_FILE="$RUN_DIR/{finished_at_file}"
  CURRENT_STATUS="$(tr -d '[:space:]' < "$STATUS_FILE" 2>/dev/null || true)"

  if [ -f "$LAUNCHER_PID_FILE" ]; then
    LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
    kill_if_running "$LAUNCHER_PID"
  fi

  if [ -f "$CODEX_PID_FILE" ]; then
    CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
    kill_if_running "$CODEX_PID"
  fi

  if [ -d "$RUN_DIR" ] && {{ [ "$CURRENT_STATUS" = "preparing" ] || [ "$CURRENT_STATUS" = "running" ]; }}; then
    printf 'canceled\n' > "$STATUS_FILE"
    date -u +%Y-%m-%dT%H:%M:%SZ > "$FINISHED_AT_FILE"
  fi
done

for RAW_WORKTREE_PATH in "${{WORKTREE_PATHS[@]}}"; do
  WORKTREE_PATH="$(expand_remote_path "$RAW_WORKTREE_PATH")"
  HAD_WORKTREE_PATH="false"
  if [ -e "$WORKTREE_PATH" ]; then
    HAD_WORKTREE_PATH="true"
  fi

  if [ -d "$CHECKOUT_PATH/.git" ] && worktree_is_registered "$WORKTREE_PATH"; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  fi

  if [ -e "$WORKTREE_PATH" ]; then
    rm -rf "$WORKTREE_PATH"
  fi

  if [ "$HAD_WORKTREE_PATH" = "true" ] && [ ! -e "$WORKTREE_PATH" ]; then
    WORKTREES_REMOVED=$((WORKTREES_REMOVED + 1))
  fi
done

if [ -d "$CHECKOUT_PATH/.git" ]; then
  git -C "$CHECKOUT_PATH" worktree prune >&2 || true
fi

if [ "{cleanup_remote_dispatch_directories}" = "true" ]; then
  for RAW_RUN_DIR in "${{RUN_DIRECTORIES[@]}}"; do
    RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
    HAD_RUN_DIRECTORY="false"
    if [ -e "$RUN_DIR" ]; then
      HAD_RUN_DIRECTORY="true"
    fi
    if [ -e "$RUN_DIR" ]; then
      rm -rf "$RUN_DIR"
    fi
    if [ "$HAD_RUN_DIRECTORY" = "true" ] && [ ! -e "$RUN_DIR" ]; then
      RUN_DIRECTORIES_REMOVED=$((RUN_DIRECTORIES_REMOVED + 1))
    fi
  done
fi

printf '{{"worktreesRemoved":%s,"runDirectoriesRemoved":%s}}\n' \
  "$WORKTREES_REMOVED" \
  "$RUN_DIRECTORIES_REMOVED"
"#,
            path_helpers = remote_path_helpers_shell(),
            cleanup_remote_dispatch_directories = if cleanup_remote_dispatch_directories {
                "true"
            } else {
                "false"
            },
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
            status_file = REMOTE_STATUS_FILE_NAME,
            finished_at_file = REMOTE_FINISHED_AT_FILE_NAME,
        );

        let mut arguments = vec![checkout_path.to_owned()];
        arguments.extend(worktree_paths.iter().cloned());
        arguments.push("--".to_owned());
        arguments.extend(run_directories.iter().cloned());
        let report = self.run_script(&cleanup_script, &arguments)?;
        parse_remote_cleanup_counts(&report)
    }

    fn cleanup_review_artifacts(
        &self,
        checkout_path: &str,
        branch_names: &[String],
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Result<(), TrackError> {
        let cleanup_script = format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
shift

BRANCH_NAMES=()
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--worktrees" ]; then
    shift
    break
  fi

  BRANCH_NAMES+=("$1")
  shift
done

WORKTREE_PATHS=()
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--runs" ]; then
    shift
    break
  fi

  WORKTREE_PATHS+=("$1")
  shift
done

RUN_DIRECTORIES=("$@")

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

worktree_is_registered() {{
  TARGET_WORKTREE="$1"
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $TARGET_WORKTREE" >/dev/null 2>&1
}}

for RAW_RUN_DIR in "${{RUN_DIRECTORIES[@]}}"; do
  RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
  LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
  CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"

  if [ -f "$LAUNCHER_PID_FILE" ]; then
    LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
    kill_if_running "$LAUNCHER_PID"
  fi

  if [ -f "$CODEX_PID_FILE" ]; then
    CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
    kill_if_running "$CODEX_PID"
  fi

  if [ -e "$RUN_DIR" ]; then
    rm -rf "$RUN_DIR"
  fi
done

for RAW_WORKTREE_PATH in "${{WORKTREE_PATHS[@]}}"; do
  WORKTREE_PATH="$(expand_remote_path "$RAW_WORKTREE_PATH")"

  if [ -d "$CHECKOUT_PATH/.git" ] && worktree_is_registered "$WORKTREE_PATH"; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  fi

  if [ -e "$WORKTREE_PATH" ]; then
    rm -rf "$WORKTREE_PATH"
  fi
done

for BRANCH_NAME in "${{BRANCH_NAMES[@]}}"; do
  if [ -d "$CHECKOUT_PATH/.git" ]; then
    git -C "$CHECKOUT_PATH" branch -D "$BRANCH_NAME" >&2 || true
  fi
done

if [ -d "$CHECKOUT_PATH/.git" ]; then
  git -C "$CHECKOUT_PATH" worktree prune >&2 || true
fi
"#,
            path_helpers = remote_path_helpers_shell(),
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
        );

        let mut arguments = vec![checkout_path.to_owned()];
        arguments.extend(branch_names.iter().cloned());
        arguments.push("--worktrees".to_owned());
        arguments.extend(worktree_paths.iter().cloned());
        arguments.push("--runs".to_owned());
        arguments.extend(run_directories.iter().cloned());
        self.run_script(&cleanup_script, &arguments)?;

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
        let cleanup_script = format!(
            r#"
set -eu
{path_helpers}
WORKSPACE_ROOT="$(expand_remote_path "$1")"
shift

KEEP_WORKTREE_PATHS=()
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--" ]; then
    shift
    break
  fi

  KEEP_WORKTREE_PATHS+=("$(expand_remote_path "$1")")
  shift
done

KEEP_RUN_DIRECTORIES=()
for RAW_RUN_DIR in "$@"; do
  KEEP_RUN_DIRECTORIES+=("$(expand_remote_path "$RAW_RUN_DIR")")
done

WORKTREES_REMOVED=0
RUN_DIRECTORIES_REMOVED=0

path_is_kept() {{
  TARGET_PATH="$1"
  shift

  for KEPT_PATH in "$@"; do
    if [ "$KEPT_PATH" = "$TARGET_PATH" ]; then
      return 0
    fi
  done

  return 1
}}

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

remove_run_directory() {{
  RUN_DIR="$1"
  LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
  CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"

  if [ -f "$LAUNCHER_PID_FILE" ]; then
    LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
    kill_if_running "$LAUNCHER_PID"
  fi

  if [ -f "$CODEX_PID_FILE" ]; then
    CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
    kill_if_running "$CODEX_PID"
  fi

  if [ -e "$RUN_DIR" ]; then
    rm -rf "$RUN_DIR"
  fi

  if [ ! -e "$RUN_DIR" ]; then
    RUN_DIRECTORIES_REMOVED=$((RUN_DIRECTORIES_REMOVED + 1))
  fi
}}

remove_worktree_path() {{
  WORKTREE_PATH="$1"
  PROJECT_DIRECTORY="$(dirname "$(dirname "$WORKTREE_PATH")")"
  PROJECT_NAME="$(basename "$PROJECT_DIRECTORY")"
  CHECKOUT_PATH="$PROJECT_DIRECTORY/$PROJECT_NAME"

  if [ -d "$CHECKOUT_PATH/.git" ]; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
    git -C "$CHECKOUT_PATH" worktree prune >&2 || true
  fi

  if [ -e "$WORKTREE_PATH" ]; then
    rm -rf "$WORKTREE_PATH"
  fi

  if [ ! -e "$WORKTREE_PATH" ]; then
    WORKTREES_REMOVED=$((WORKTREES_REMOVED + 1))
  fi
}}

for PROJECT_DIRECTORY in "$WORKSPACE_ROOT"/*; do
  [ -d "$PROJECT_DIRECTORY" ] || continue

  for RUN_DIR in "$PROJECT_DIRECTORY"/dispatches/dispatch-*; do
    [ -e "$RUN_DIR" ] || continue
    if path_is_kept "$RUN_DIR" "${{KEEP_RUN_DIRECTORIES[@]}}"; then
      continue
    fi

    remove_run_directory "$RUN_DIR"
  done

  for WORKTREE_PATH in "$PROJECT_DIRECTORY"/worktrees/dispatch-*; do
    [ -e "$WORKTREE_PATH" ] || continue
    if path_is_kept "$WORKTREE_PATH" "${{KEEP_WORKTREE_PATHS[@]}}"; then
      continue
    fi

    remove_worktree_path "$WORKTREE_PATH"
  done

  for RUN_DIR in "$PROJECT_DIRECTORY"/{review_run_directory}/dispatch-*; do
    [ -e "$RUN_DIR" ] || continue
    if path_is_kept "$RUN_DIR" "${{KEEP_RUN_DIRECTORIES[@]}}"; then
      continue
    fi

    remove_run_directory "$RUN_DIR"
  done

  for WORKTREE_PATH in "$PROJECT_DIRECTORY"/{review_worktree_directory}/dispatch-*; do
    [ -e "$WORKTREE_PATH" ] || continue
    if path_is_kept "$WORKTREE_PATH" "${{KEEP_WORKTREE_PATHS[@]}}"; then
      continue
    fi

    remove_worktree_path "$WORKTREE_PATH"
  done
done

printf '{{"worktreesRemoved":%s,"runDirectoriesRemoved":%s}}\n' \
  "$WORKTREES_REMOVED" \
  "$RUN_DIRECTORIES_REMOVED"
"#,
            path_helpers = remote_path_helpers_shell(),
            review_run_directory = REVIEW_RUN_DIRECTORY_NAME,
            review_worktree_directory = REVIEW_WORKTREE_DIRECTORY_NAME,
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
        );

        let mut arguments = vec![workspace_root.to_owned()];
        arguments.extend(kept_worktree_paths.iter().cloned());
        arguments.push("--".to_owned());
        arguments.extend(kept_run_directories.iter().cloned());
        let report = self.run_script(&cleanup_script, &arguments)?;
        parse_remote_cleanup_counts(&report)
    }

    fn cleanup_review_workspace_caches(&self, checkout_paths: &[String]) -> Result<(), TrackError> {
        if checkout_paths.is_empty() {
            return Ok(());
        }

        let cleanup_script = format!(
            r#"
set -eu
{path_helpers}

for RAW_CHECKOUT_PATH in "$@"; do
  CHECKOUT_PATH="$(expand_remote_path "$RAW_CHECKOUT_PATH")"
  WORKSPACE_PATH="$(dirname "$CHECKOUT_PATH")"

  if [ -d "$CHECKOUT_PATH/.git" ]; then
    git -C "$CHECKOUT_PATH" worktree prune >&2 || true
  fi

  if [ -e "$CHECKOUT_PATH" ]; then
    rm -rf "$CHECKOUT_PATH"
  fi

  if [ -d "$WORKSPACE_PATH" ]; then
    rmdir "$WORKSPACE_PATH" 2>/dev/null || true
  fi
done
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(&cleanup_script, checkout_paths)?;

        Ok(())
    }

    fn reset_workspace(
        &self,
        workspace_root: &str,
        projects_registry_path: &str,
    ) -> Result<RemoteResetSummary, TrackError> {
        let reset_script = format!(
            r#"
set -eu
{path_helpers}
WORKSPACE_ROOT="$(expand_remote_path "$1")"
REGISTRY_PATH="$(expand_remote_path "$2")"
WORKSPACE_ENTRIES_REMOVED=0
REGISTRY_REMOVED=false

if [ -z "$WORKSPACE_ROOT" ] || [ "$WORKSPACE_ROOT" = "/" ] || [ "$WORKSPACE_ROOT" = "$HOME" ]; then
  echo "Refusing to reset an unsafe remote workspace root at $WORKSPACE_ROOT." >&2
  exit 1
fi

mkdir -p "$WORKSPACE_ROOT"

for ENTRY in "$WORKSPACE_ROOT"/* "$WORKSPACE_ROOT"/.[!.]* "$WORKSPACE_ROOT"/..?*; do
  [ -e "$ENTRY" ] || continue
  rm -rf "$ENTRY"
  if [ ! -e "$ENTRY" ]; then
    WORKSPACE_ENTRIES_REMOVED=$((WORKSPACE_ENTRIES_REMOVED + 1))
  fi
done

if [ -e "$REGISTRY_PATH" ]; then
  rm -f "$REGISTRY_PATH"
  if [ ! -e "$REGISTRY_PATH" ]; then
    REGISTRY_REMOVED=true
  fi
fi

printf '{{"workspaceEntriesRemoved":%s,"registryRemoved":%s}}\n' \
  "$WORKSPACE_ENTRIES_REMOVED" \
  "$REGISTRY_REMOVED"
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        let report = self.run_script(
            &reset_script,
            &[workspace_root.to_owned(), projects_registry_path.to_owned()],
        )?;
        parse_remote_reset_summary(&report)
    }

    fn read_remote_file(&self, remote_path: &str) -> Result<Option<String>, TrackError> {
        let read_remote_file_script = format!(
            r#"
set -eu
{path_helpers}
REMOTE_PATH="$(expand_remote_path "$1")"
if [ -f "$REMOTE_PATH" ]; then
  cat "$REMOTE_PATH"
else
  exit 3
fi
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        match self.run_script_with_exit_code(&read_remote_file_script, &[remote_path.to_owned()])? {
            ScriptOutput::Success(stdout) => Ok(Some(stdout)),
            ScriptOutput::ExitCode(3) => Ok(None),
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
        let upload_remote_file_script = format!(
            r#"
set -eu
{path_helpers}
REMOTE_PATH="$(expand_remote_path "$1")"
mkdir -p "$(dirname "$REMOTE_PATH")"
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(&upload_remote_file_script, &[remote_path.to_owned()])?;

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

        let snapshot_script = format!(
            r#"
set -eu
{path_helpers}

emit_file() {{
  LABEL="$1"
  FILE_PATH="$(expand_remote_path "$2")"

  printf '%s\t' "$LABEL"
  if [ -f "$FILE_PATH" ]; then
    printf 'present\t'
    od -An -tx1 -v "$FILE_PATH" | tr -d ' \n'
  else
    printf 'missing\t'
  fi
  printf '\n'
}}

for RAW_RUN_DIR in "$@"; do
  RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
  printf 'run\t%s\n' "$RAW_RUN_DIR"
  emit_file "status" "$RUN_DIR/{status_file}"
  emit_file "result" "$RUN_DIR/{result_file}"
  emit_file "stderr" "$RUN_DIR/{stderr_file}"
  emit_file "finished_at" "$RUN_DIR/{finished_at_file}"
done
"#,
            path_helpers = remote_path_helpers_shell(),
            status_file = REMOTE_STATUS_FILE_NAME,
            result_file = REMOTE_RESULT_FILE_NAME,
            stderr_file = REMOTE_STDERR_FILE_NAME,
            finished_at_file = REMOTE_FINISHED_AT_FILE_NAME,
        );
        let report = self.run_script(&snapshot_script, run_directories)?;

        parse_dispatch_snapshot_report(&report)
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

fn parse_dispatch_snapshot_report(report: &str) -> Result<Vec<RemoteDispatchSnapshot>, TrackError> {
    let mut snapshots = Vec::new();
    let mut current_snapshot: Option<RemoteDispatchSnapshot> = None;

    for line in report.lines().filter(|line| !line.trim().is_empty()) {
        let columns = line.splitn(3, '\t').collect::<Vec<_>>();
        match columns.first().copied() {
            Some("run") => {
                let _run_identifier = columns.get(1).ok_or_else(|| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        "Remote dispatch refresh report is missing a run directory.",
                    )
                })?;
                if let Some(snapshot) = current_snapshot.take() {
                    snapshots.push(snapshot);
                }
                current_snapshot = Some(RemoteDispatchSnapshot::default());
            }
            Some("status") | Some("result") | Some("stderr") | Some("finished_at") => {
                let field_name = columns
                    .first()
                    .expect("field-tagged dispatch line should have a tag");
                let presence = columns.get(1).ok_or_else(|| {
                    TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        "Remote dispatch refresh report is missing a field state.",
                    )
                })?;
                let value = match *presence {
                    "missing" => None,
                    "present" => Some(decode_hex_string(columns.get(2).copied().unwrap_or(""))?),
                    _ => {
                        return Err(TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            "Remote dispatch refresh report has an unknown field state.",
                        ));
                    }
                };
                let Some(snapshot) = current_snapshot.as_mut() else {
                    return Err(TrackError::new(
                        ErrorCode::RemoteDispatchFailed,
                        "Remote dispatch refresh report emitted a field before the run header.",
                    ));
                };
                match *field_name {
                    "status" => snapshot.status = value,
                    "result" => snapshot.result = value,
                    "stderr" => snapshot.stderr = value,
                    "finished_at" => snapshot.finished_at = value,
                    _ => {}
                }
            }
            _ => {
                return Err(TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    "Remote dispatch refresh report contains an unexpected line.",
                ));
            }
        }
    }

    if let Some(snapshot) = current_snapshot {
        snapshots.push(snapshot);
    }

    Ok(snapshots)
}

fn parse_remote_cleanup_counts(report: &str) -> Result<RemoteArtifactCleanupCounts, TrackError> {
    let parsed_report = serde_json::from_str::<RemoteArtifactCleanupReport>(report.trim())
        .map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not parse the remote cleanup report: {error}"),
            )
        })?;

    Ok(RemoteArtifactCleanupCounts {
        worktrees_removed: parsed_report.worktrees_removed,
        run_directories_removed: parsed_report.run_directories_removed,
    })
}

fn parse_remote_reset_summary(report: &str) -> Result<RemoteResetSummary, TrackError> {
    let parsed_report =
        serde_json::from_str::<RemoteWorkspaceResetReport>(report.trim()).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not parse the remote reset report: {error}"),
            )
        })?;

    Ok(RemoteResetSummary {
        workspace_entries_removed: parsed_report.workspace_entries_removed,
        registry_removed: parsed_report.registry_removed,
    })
}

fn decode_hex_string(hex: &str) -> Result<String, TrackError> {
    if !hex.len().is_multiple_of(2) {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            "Remote dispatch refresh data is not valid hexadecimal.",
        ));
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut index = 0;
    while index < hex.len() {
        let byte = u8::from_str_radix(&hex[index..index + 2], 16).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Remote dispatch refresh data is not valid hexadecimal: {error}"),
            )
        })?;
        bytes.push(byte);
        index += 2;
    }

    String::from_utf8(bytes).map_err(|error| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Remote dispatch refresh data is not valid UTF-8: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use serde_json::json;
    use tempfile::TempDir;

    use crate::backend_config::RemoteAgentConfigService;
    use crate::config::{
        ApiConfigFile, LlamaCppConfigFile, RemoteAgentConfigFile, TrackConfigFile,
    };
    use crate::dispatch_repository::DispatchRepository;
    use crate::project_repository::{ProjectMetadata, ProjectRepository};
    use crate::review_dispatch_repository::ReviewDispatchRepository;
    use crate::review_repository::ReviewRepository;
    use crate::task_description::render_task_description;
    use crate::task_repository::FileTaskRepository;
    use crate::test_support::{set_env_var, track_data_env_lock};
    use crate::time_utils::{now_utc, parse_iso_8601_seconds};
    use crate::types::{
        DispatchStatus, Priority, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord, Status,
        TaskCreateInput, TaskDispatchRecord, TaskSource, TaskUpdateInput,
    };
    use time::Duration;

    use super::{
        build_create_review_worktree_script, build_remote_agent_launcher,
        build_remote_dispatch_prompt, build_remote_dispatch_schema, build_remote_review_prompt,
        build_remote_review_schema, build_review_follow_up_request, build_review_workspace_key,
        describe_remote_reset_blockers, latest_pull_request_for_branch,
        parse_dispatch_snapshot_report, parse_github_pull_request_reference,
        parse_github_repository_name, refresh_dispatch_record_from_snapshot,
        render_remote_script_with_shell_prelude, select_follow_up_base_dispatch,
        select_previous_submitted_review_run, GithubPullRequestMetadata, RemoteDispatchService,
        RemoteDispatchSnapshot, RemoteReviewService,
    };

    struct TestContext {
        _directory: TempDir,
        _env_lock_guard: std::sync::MutexGuard<'static, ()>,
        _track_state_dir_guard: crate::test_support::EnvVarGuard,
        data_dir: PathBuf,
        config_service: RemoteAgentConfigService,
        dispatch_repository: DispatchRepository,
        project_repository: ProjectRepository,
        review_dispatch_repository: ReviewDispatchRepository,
        review_repository: ReviewRepository,
        task_repository: FileTaskRepository,
    }

    impl TestContext {
        fn new(config: TrackConfigFile) -> Self {
            let directory = TempDir::new().expect("tempdir should be created");
            let env_lock_guard = track_data_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state_root = directory.path().join("state");
            let track_state_dir_guard = set_env_var("TRACK_STATE_DIR", &state_root);
            let data_dir = state_root.join("issues");
            let database_path = state_root.join("track.sqlite");
            let config_service =
                RemoteAgentConfigService::new(None).expect("config service should resolve");
            config_service
                .save_remote_agent_config(config.remote_agent.as_ref())
                .expect("remote-agent config should save");

            Self {
                _directory: directory,
                _env_lock_guard: env_lock_guard,
                _track_state_dir_guard: track_state_dir_guard,
                data_dir: data_dir.clone(),
                config_service,
                dispatch_repository: DispatchRepository::new(Some(database_path.clone()))
                    .expect("dispatch repository should resolve"),
                project_repository: ProjectRepository::new(Some(database_path.clone()))
                    .expect("project repository should resolve"),
                review_dispatch_repository: ReviewDispatchRepository::new(Some(
                    database_path.clone(),
                ))
                .expect("review dispatch repository should resolve"),
                review_repository: ReviewRepository::new(Some(database_path.clone()))
                    .expect("review repository should resolve"),
                task_repository: FileTaskRepository::new(Some(database_path))
                    .expect("task repository should resolve"),
            }
        }

        fn service(&self) -> RemoteDispatchService<'_> {
            RemoteDispatchService {
                config_service: &self.config_service,
                dispatch_repository: &self.dispatch_repository,
                project_repository: &self.project_repository,
                task_repository: &self.task_repository,
                review_repository: &self.review_repository,
                review_dispatch_repository: &self.review_dispatch_repository,
            }
        }

        fn review_service(&self) -> RemoteReviewService<'_> {
            RemoteReviewService {
                config_service: &self.config_service,
                project_repository: &self.project_repository,
                review_repository: &self.review_repository,
                review_dispatch_repository: &self.review_dispatch_repository,
            }
        }

        fn create_task(&self, project: &str, description: &str) -> crate::types::Task {
            self.project_repository
                .upsert_project_by_name(
                    project,
                    ProjectMetadata {
                        repo_url: format!("https://github.com/acme/{project}"),
                        git_url: format!("git@github.com:acme/{project}.git"),
                        base_branch: "main".to_owned(),
                        description: None,
                    },
                    Vec::new(),
                )
                .expect("project should save");
            self.task_repository
                .create_task(TaskCreateInput {
                    project: project.to_owned(),
                    priority: Priority::High,
                    description: description.to_owned(),
                    source: Some(TaskSource::Web),
                })
                .expect("task should be created")
                .task
        }

        fn write_project_metadata(&self, project: &str) {
            self.project_repository
                .upsert_project_by_name(
                    project,
                    ProjectMetadata {
                        repo_url: format!("https://github.com/acme/{project}"),
                        git_url: format!("git@github.com:acme/{project}.git"),
                        base_branch: "main".to_owned(),
                        description: None,
                    },
                    Vec::new(),
                )
                .expect("project metadata should save");
        }

        fn create_running_dispatch(&self, task: &crate::types::Task) -> TaskDispatchRecord {
            let mut dispatch = self
                .dispatch_repository
                .create_dispatch(task, "198.51.100.10", RemoteAgentPreferredTool::Codex)
                .expect("dispatch should be created");
            dispatch.status = DispatchStatus::Running;
            dispatch.branch_name = Some(format!("track/{}", dispatch.dispatch_id));
            dispatch.worktree_path = Some(format!(
                "~/workspace/{}/worktrees/{}",
                task.project, dispatch.dispatch_id
            ));
            dispatch.summary =
                Some("The remote agent is working in the prepared environment.".to_owned());
            dispatch.updated_at = now_utc();
            self.dispatch_repository
                .save_dispatch(&dispatch)
                .expect("dispatch should save");
            dispatch
        }

        fn create_review(&self) -> ReviewRecord {
            let review = sample_review_record();
            self.review_repository
                .save_review(&review)
                .expect("review should save");
            review
        }

        fn create_review_run(
            &self,
            review: &ReviewRecord,
            status: DispatchStatus,
        ) -> ReviewRunRecord {
            let timestamp = now_utc();
            let dispatch_id = format!("dispatch-{}", timestamp.unix_timestamp_nanos());
            let record = ReviewRunRecord {
                dispatch_id: dispatch_id.clone(),
                review_id: review.id.clone(),
                pull_request_url: review.pull_request_url.clone(),
                repository_full_name: review.repository_full_name.clone(),
                workspace_key: review.workspace_key.clone(),
                preferred_tool: review.preferred_tool,
                status,
                created_at: timestamp,
                updated_at: timestamp,
                finished_at: None,
                remote_host: "198.51.100.10".to_owned(),
                branch_name: Some(format!("track-review/{dispatch_id}")),
                worktree_path: Some(format!(
                    "~/workspace/{}/{}/{}",
                    review.workspace_key,
                    super::REVIEW_WORKTREE_DIRECTORY_NAME,
                    dispatch_id
                )),
                follow_up_request: None,
                target_head_oid: Some("abc123def456".to_owned()),
                summary: None,
                review_submitted: false,
                github_review_id: None,
                github_review_url: None,
                notes: None,
                error_message: None,
            };
            self.review_dispatch_repository
                .save_dispatch(&record)
                .expect("review run should save");

            record
        }
    }

    fn base_test_config(remote_agent: Option<RemoteAgentConfigFile>) -> TrackConfigFile {
        TrackConfigFile {
            project_roots: Vec::new(),
            project_aliases: BTreeMap::new(),
            api: ApiConfigFile { port: 3210 },
            llama_cpp: LlamaCppConfigFile {
                model_path: Some("/tmp/parser.gguf".to_owned()),
                model_hf_repo: None,
                model_hf_file: None,
            },
            remote_agent,
        }
    }

    fn install_dummy_managed_remote_agent_material(data_dir: &Path) {
        let remote_agent_dir = data_dir
            .parent()
            .expect("data dir should have a parent")
            .join("remote-agent");
        fs::create_dir_all(&remote_agent_dir).expect("remote-agent dir should be created");
        fs::write(
            remote_agent_dir.join("id_ed25519"),
            "not-a-real-private-key",
        )
        .expect("dummy SSH key should be written");
        fs::write(remote_agent_dir.join("known_hosts"), "")
            .expect("dummy known_hosts file should be written");
    }

    fn sample_review_record() -> ReviewRecord {
        let created_at = now_utc();
        ReviewRecord {
            id: "20260326-120000-review-pr-42".to_owned(),
            pull_request_url: "https://github.com/acme/project-x/pull/42".to_owned(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-x".to_owned(),
            repo_url: "https://github.com/acme/project-x".to_owned(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            workspace_key: "project-x".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: Some("project-x".to_owned()),
            main_user: "octocat".to_owned(),
            default_review_prompt: Some("Focus on regressions and missing tests.".to_owned()),
            extra_instructions: Some("Pay special attention to queue rendering.".to_owned()),
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn builds_remote_prompt_with_both_summary_layers() {
        let prompt = build_remote_dispatch_prompt(
            "project-x",
            &ProjectMetadata {
                repo_url: "https://github.com/acme/project-x".to_owned(),
                git_url: "git@github.com:acme/project-x.git".to_owned(),
                base_branch: "main".to_owned(),
                description: Some("Main repo".to_owned()),
            },
            "track/dispatch-1",
            "~/workspace/project-x/worktrees/dispatch-1",
            &render_task_description(
                "Fix a bug in module A",
                Some("- Inspect `module_a.rs`"),
                Some("proj-x prio high fix a bug in module A"),
            ),
            Some("https://github.com/acme/project-x/pull/42"),
            Some("Address review comments from the latest PR review."),
        );

        assert!(prompt.contains("## Summary"));
        assert!(prompt.contains("## Original note"));
        assert!(prompt.contains("## Existing PR"));
        assert!(prompt.contains("## Current follow-up request"));
        assert!(prompt.contains("fetch that context with `gh`"));
        assert!(prompt.contains("only act on that reviewer's feedback"));
        assert!(prompt.contains("track/dispatch-1"));
        assert!(
            prompt.contains("Use conventional commits for both commit messages and the PR title")
        );
    }

    #[test]
    fn dispatch_schema_limits_terminal_status_values() {
        let schema = build_remote_dispatch_schema();

        assert!(schema.contains("\"succeeded\""));
        assert!(schema.contains("\"failed\""));
        assert!(schema.contains("\"blocked\""));
        assert!(schema.contains("\"pullRequestUrl\""));
        assert!(schema.contains("\"branchName\""));
        assert!(schema.contains("\"notes\""));
        assert!(schema.contains("\"required\""));
        assert!(!schema.contains("\"running\""));
    }

    #[test]
    fn builds_remote_review_prompt_with_follow_up_guidance_and_saved_context() {
        let review = sample_review_record();
        let previous_review_run = ReviewRunRecord {
            dispatch_id: "review-dispatch-1".to_owned(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Succeeded,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: Some(now_utc()),
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some("track-review/review-dispatch-1".to_owned()),
            worktree_path: Some(
                "~/workspace/project-x/review-worktrees/review-dispatch-1".to_owned(),
            ),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: Some("Submitted a GitHub review with two inline comments.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(
                "https://github.com/acme/project-x/pull/42#pullrequestreview-1001".to_owned(),
            ),
            notes: None,
            error_message: None,
        };
        let current_review_run = ReviewRunRecord {
            dispatch_id: "review-dispatch-2".to_owned(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Preparing,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some("track-review/review-dispatch-2".to_owned()),
            worktree_path: Some(
                "~/workspace/project-x/review-worktrees/review-dispatch-2".to_owned(),
            ),
            follow_up_request: Some(
                "Check whether the main review comments were actually resolved.".to_owned(),
            ),
            target_head_oid: Some("fedcba654321".to_owned()),
            summary: Some(
                "Re-review request: Check whether the main review comments were actually resolved."
                    .to_owned(),
            ),
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };
        let prompt =
            build_remote_review_prompt(&review, &current_review_run, Some(&previous_review_run));

        assert!(prompt.contains("You are responsible for submitting the GitHub review yourself"));
        assert!(prompt.contains("Submit one GitHub review in COMMENT mode."));
        assert!(prompt.contains("Prefer inline review comments"));
        assert!(prompt.contains("The first line of the top-level review body must be `@octocat requested me to review this PR.`"));
        assert!(prompt.contains("- Pinned review commit: fedcba654321"));
        assert!(prompt.contains("the prepared worktree is intended to match that exact commit"));
        assert!(prompt.contains("Capture the submitted GitHub review's durable handle"));
        assert!(prompt.contains("Return `reviewSubmitted` as `true` only after GitHub confirms"));
        assert!(prompt.contains("## Current re-review request"));
        assert!(prompt.contains("Check whether the main review comments were actually resolved."));
        assert!(prompt.contains("## Previous bot review context"));
        assert!(prompt.contains("https://github.com/acme/project-x/pull/42#pullrequestreview-1001"));
        assert!(prompt.contains("## Re-review guidance"));
        assert!(prompt.contains("non-blocking input at the discretion of the reviewee unless @octocat explicitly commented"));
        assert!(prompt.contains("do not repeat it as a primary finding"));
        assert!(prompt.contains("## Default review prompt"));
        assert!(prompt.contains("Focus on regressions and missing tests."));
        assert!(prompt.contains("## Extra instructions"));
        assert!(prompt.contains("Pay special attention to queue rendering."));
    }

    #[test]
    fn review_worktree_script_pins_the_requested_commit_or_fails_explicitly() {
        let script = build_create_review_worktree_script();

        assert!(script.contains("TARGET_HEAD_OID"));
        assert!(script.contains("fetch upstream \"$TARGET_HEAD_OID\""));
        assert!(script.contains("TARGET_REF=\"$TARGET_HEAD_OID\""));
        assert!(
            script.contains("Requested review commit $TARGET_HEAD_OID is not available locally.")
        );
        assert!(script.contains("review would drift to a newer commit"));
        assert!(script.contains("branch -f \"$BRANCH_NAME\" \"$TARGET_REF\""));
    }

    #[test]
    fn review_schema_requires_review_submission_metadata_and_terminal_status_values() {
        let schema = build_remote_review_schema();

        assert!(schema.contains("\"reviewSubmitted\""));
        assert!(schema.contains("\"githubReviewId\""));
        assert!(schema.contains("\"githubReviewUrl\""));
        assert!(schema.contains("\"succeeded\""));
        assert!(schema.contains("\"failed\""));
        assert!(schema.contains("\"blocked\""));
        assert!(!schema.contains("\"running\""));
    }

    #[test]
    fn parses_github_repository_name() {
        assert_eq!(
            parse_github_repository_name("https://github.com/acme/project-x")
                .expect("github url should parse"),
            "project-x"
        );
    }

    #[test]
    fn parses_github_pull_request_reference() {
        let reference =
            parse_github_pull_request_reference("https://github.com/acme/project-x/pull/42")
                .expect("github pr url should parse");

        assert_eq!(reference.owner, "acme");
        assert_eq!(reference.repository, "project-x");
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn builds_review_workspace_key_from_repository_name() {
        let metadata = GithubPullRequestMetadata {
            pull_request_url: "https://github.com/acme/project-x/pull/42".to_owned(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-x".to_owned(),
            repo_url: "https://github.com/acme/project-x".to_owned(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            head_oid: "abc123".to_owned(),
        };

        assert_eq!(build_review_workspace_key(&metadata), "acme-project-x");
    }

    #[test]
    fn builds_review_follow_up_request_that_scopes_feedback_to_one_user() {
        let request = build_review_follow_up_request(
            "https://github.com/acme/project-x/pull/42",
            "octocat",
            parse_iso_8601_seconds("2026-03-25T12:00:00Z").expect("timestamp should parse"),
        );

        assert!(request.contains("@octocat"));
        assert!(request.contains("COMMENTED or CHANGES_REQUESTED"));
        assert!(request.contains("Ignore APPROVED reviews"));
        assert!(request.contains("https://github.com/acme/project-x/pull/42"));
    }

    #[test]
    fn saved_review_dispatch_prerequisites_do_not_depend_on_live_review_follow_up_settings() {
        let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
            host: "127.0.0.1".to_owned(),
            user: "builder".to_owned(),
            port: 2222,
            workspace_root: "~/workspace".to_owned(),
            projects_registry_path: "~/track-projects.json".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
            review_follow_up: None,
        })));
        let review = context.create_review();

        let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
        install_dummy_managed_remote_agent_material(&context.data_dir);

        let (remote_agent, loaded_review) = context
            .review_service()
            .load_review_dispatch_prerequisites(&review.id)
            .expect("saved review dispatch prerequisites should load");

        assert_eq!(remote_agent.host, "127.0.0.1");
        assert_eq!(loaded_review.id, review.id);
        assert_eq!(loaded_review.main_user, review.main_user);
        assert_eq!(
            loaded_review.default_review_prompt,
            review.default_review_prompt
        );
    }

    #[test]
    fn parses_batched_dispatch_snapshot_report() {
        let report = concat!(
            "run\t~/workspace/project-x/dispatches/dispatch-1\n",
            "status\tpresent\t72756e6e696e670a\n",
            "result\tmissing\t\n",
            "stderr\tmissing\t\n",
            "finished_at\tmissing\t\n",
            "run\t~/workspace/project-y/dispatches/dispatch-2\n",
            "status\tpresent\t636f6d706c657465640a\n",
            "result\tpresent\t7b22737461747573223a22737563636565646564227d\n",
            "stderr\tpresent\t\n",
            "finished_at\tpresent\t323032362d30332d31385431303a33353a33315a0a\n",
        );

        let snapshots =
            parse_dispatch_snapshot_report(report).expect("dispatch snapshot report should parse");

        assert_eq!(
            snapshots
                .first()
                .expect("first dispatch snapshot should exist")
                .status
                .as_deref(),
            Some("running\n")
        );
        assert_eq!(
            snapshots
                .get(1)
                .expect("second dispatch snapshot should exist")
                .result
                .as_deref(),
            Some("{\"status\":\"succeeded\"}")
        );
        assert_eq!(
            snapshots
                .get(1)
                .expect("second dispatch snapshot should exist")
                .finished_at
                .as_deref(),
            Some("2026-03-18T10:35:31Z\n")
        );
    }

    #[test]
    fn prepends_shell_prelude_before_remote_script_body() {
        let rendered = render_remote_script_with_shell_prelude(
            "set -eu\nprintf '%s\\n' done\n",
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"\n",
        );

        assert!(rendered.starts_with("set -e\n"));
        assert!(rendered.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(rendered.contains(". \"$HOME/.cargo/env\""));
        assert!(rendered.contains("printf '%s\\n' done"));
    }

    #[test]
    fn builds_codex_launcher_with_runner_shell_prelude() {
        let launcher = build_remote_agent_launcher(
            RemoteAgentPreferredTool::Codex,
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"\n",
        );

        assert!(launcher.starts_with("#!/usr/bin/env bash"));
        assert!(launcher.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(launcher.contains("codex --search exec"));
        assert!(launcher.contains("RUN_DIR=\"$1\""));
        assert!(launcher.contains("WORKTREE_PATH=\"$2\""));
        assert!(launcher.contains("launcher.pid"));
        assert!(launcher.contains("codex.pid"));
        assert!(launcher.contains("trap cancel_run TERM INT"));
        assert!(launcher.contains("canceled"));
    }

    #[test]
    fn builds_claude_launcher_with_schema_validation_and_yolo_mode() {
        let launcher = build_remote_agent_launcher(
            RemoteAgentPreferredTool::Claude,
            "export PATH=\"$HOME/.local/bin:$PATH\"\n",
        );

        assert!(launcher.starts_with("#!/usr/bin/env bash"));
        assert!(launcher.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
        assert!(launcher.contains("SCHEMA_CONTENT=\"$(tr -d '\\n'"));
        assert!(launcher.contains("cd \"$WORKTREE_PATH\""));
        assert!(launcher.contains("claude -p --dangerously-skip-permissions"));
        assert!(launcher.contains("--output-format json"));
        assert!(launcher.contains("--json-schema \"$SCHEMA_CONTENT\""));
        assert!(launcher.contains("codex.pid"));
    }

    #[test]
    fn refresh_reads_claude_dispatch_outcome_from_structured_output_envelope() {
        let created_at = now_utc();
        let record = TaskDispatchRecord {
            dispatch_id: "dispatch-1".to_owned(),
            task_id: "task-1".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Claude,
            project: "project-a".to_owned(),
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track/dispatch-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        let snapshot = RemoteDispatchSnapshot {
            status: Some("completed\n".to_owned()),
            result: Some(
                json!({
                    "result": "Mock Claude completed successfully.",
                    "structured_output": {
                        "status": "succeeded",
                        "summary": "Mock Claude completed successfully.",
                        "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
                        "branchName": "track/dispatch-1",
                        "worktreePath": "/tmp/project-a/worktrees/dispatch-1",
                        "notes": "Captured from the Claude mock."
                    }
                })
                .to_string(),
            ),
            stderr: None,
            finished_at: Some("2026-03-18T10:35:31Z\n".to_owned()),
        };

        let refreshed = refresh_dispatch_record_from_snapshot(record, &snapshot)
            .expect("Claude envelope should refresh successfully");

        assert_eq!(refreshed.status, DispatchStatus::Succeeded);
        assert_eq!(
            refreshed.summary.as_deref(),
            Some("Mock Claude completed successfully.")
        );
        assert_eq!(
            refreshed.pull_request_url.as_deref(),
            Some("https://github.com/acme/project-a/pull/42")
        );
        assert_eq!(
            refreshed.worktree_path.as_deref(),
            Some("/tmp/project-a/worktrees/dispatch-1")
        );
        assert_eq!(
            refreshed.notes.as_deref(),
            Some("Captured from the Claude mock.")
        );
    }

    #[test]
    fn refresh_reads_claude_review_outcome_from_structured_output_envelope() {
        let context = TestContext::new(base_test_config(None));
        let created_at = now_utc();
        let record = ReviewRunRecord {
            dispatch_id: "review-dispatch-1".to_owned(),
            review_id: "review-1".to_owned(),
            pull_request_url: "https://github.com/acme/project-a/pull/42".to_owned(),
            repository_full_name: "acme/project-a".to_owned(),
            workspace_key: "project-a".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Claude,
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track-review/review-dispatch-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/review-worktrees/review-1".to_owned()),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: None,
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };
        let snapshot = RemoteDispatchSnapshot {
            status: Some("completed\n".to_owned()),
            result: Some(
                json!({
                    "result": "Mock Claude reviewed the pull request successfully.",
                    "structured_output": {
                        "status": "succeeded",
                        "summary": "Mock Claude reviewed the pull request successfully.",
                        "reviewSubmitted": true,
                        "githubReviewId": "1001",
                        "githubReviewUrl": "https://github.com/acme/project-a/pull/42#pullrequestreview-1001",
                        "worktreePath": "/tmp/project-a/review-worktrees/review-1",
                        "notes": "Captured from the Claude review mock."
                    }
                })
                .to_string(),
            ),
            stderr: None,
            finished_at: Some("2026-03-18T10:35:31Z\n".to_owned()),
        };

        let refreshed = context
            .review_service()
            .refresh_review_dispatch_record_from_snapshot(record, &snapshot)
            .expect("Claude review envelope should refresh successfully");

        assert_eq!(refreshed.status, DispatchStatus::Succeeded);
        assert_eq!(
            refreshed.summary.as_deref(),
            Some("Mock Claude reviewed the pull request successfully.")
        );
        assert!(refreshed.review_submitted);
        assert_eq!(refreshed.github_review_id.as_deref(), Some("1001"));
        assert_eq!(
            refreshed.github_review_url.as_deref(),
            Some("https://github.com/acme/project-a/pull/42#pullrequestreview-1001")
        );
        assert_eq!(
            refreshed.worktree_path.as_deref(),
            Some("/tmp/project-a/review-worktrees/review-1")
        );
        assert_eq!(
            refreshed.notes.as_deref(),
            Some("Captured from the Claude review mock.")
        );
    }

    #[test]
    fn refresh_marks_remote_canceled_runs_as_terminal() {
        let created_at = now_utc();
        let record = TaskDispatchRecord {
            dispatch_id: "dispatch-1".to_owned(),
            task_id: "task-1".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: "project-a".to_owned(),
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track/dispatch-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        let snapshot = RemoteDispatchSnapshot {
            status: Some("canceled\n".to_owned()),
            result: None,
            stderr: None,
            finished_at: Some("2026-03-18T10:35:31Z\n".to_owned()),
        };

        let refreshed = refresh_dispatch_record_from_snapshot(record, &snapshot)
            .expect("canceled snapshot should refresh");

        assert_eq!(refreshed.status, DispatchStatus::Canceled);
        assert_eq!(
            refreshed.summary.as_deref(),
            Some("Canceled from the web UI.")
        );
        assert!(refreshed.finished_at.is_some());
    }

    #[test]
    fn follow_up_uses_the_latest_reusable_dispatch_context() {
        let created_at = now_utc();
        let dispatch_history = vec![
            TaskDispatchRecord {
                dispatch_id: "dispatch-3".to_owned(),
                task_id: "task-1".to_owned(),
                preferred_tool: RemoteAgentPreferredTool::Codex,
                project: "project-a".to_owned(),
                status: DispatchStatus::Failed,
                created_at: created_at + Duration::seconds(2),
                updated_at: created_at + Duration::seconds(2),
                finished_at: Some(created_at + Duration::seconds(2)),
                remote_host: "192.0.2.25".to_owned(),
                branch_name: None,
                worktree_path: None,
                pull_request_url: None,
                follow_up_request: Some("Address review comments".to_owned()),
                summary: Some("Launch failed before the branch was restored.".to_owned()),
                notes: None,
                error_message: Some("Remote launch failed.".to_owned()),
                review_request_head_oid: None,
                review_request_user: None,
            },
            TaskDispatchRecord {
                dispatch_id: "dispatch-2".to_owned(),
                task_id: "task-1".to_owned(),
                preferred_tool: RemoteAgentPreferredTool::Claude,
                project: "project-a".to_owned(),
                status: DispatchStatus::Succeeded,
                created_at: created_at + Duration::seconds(1),
                updated_at: created_at + Duration::seconds(1),
                finished_at: Some(created_at + Duration::seconds(1)),
                remote_host: "192.0.2.25".to_owned(),
                branch_name: Some("track/dispatch-2".to_owned()),
                worktree_path: Some("~/workspace/project-a/worktrees/dispatch-2".to_owned()),
                pull_request_url: Some("https://github.com/acme/project-a/pull/42".to_owned()),
                follow_up_request: None,
                summary: Some("Opened a PR.".to_owned()),
                notes: None,
                error_message: None,
                review_request_head_oid: None,
                review_request_user: None,
            },
            TaskDispatchRecord {
                dispatch_id: "dispatch-1".to_owned(),
                task_id: "task-1".to_owned(),
                preferred_tool: RemoteAgentPreferredTool::Codex,
                project: "project-a".to_owned(),
                status: DispatchStatus::Failed,
                created_at,
                updated_at: created_at,
                finished_at: Some(created_at),
                remote_host: "192.0.2.25".to_owned(),
                branch_name: Some("track/dispatch-1".to_owned()),
                worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
                pull_request_url: Some("https://github.com/acme/project-a/pull/1".to_owned()),
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: Some("Old failure.".to_owned()),
                review_request_head_oid: None,
                review_request_user: None,
            },
        ];

        let selected = select_follow_up_base_dispatch(&dispatch_history)
            .expect("a reusable dispatch should be selected");
        let pull_request_url = latest_pull_request_for_branch(
            &dispatch_history,
            selected
                .branch_name
                .as_deref()
                .expect("selected dispatch should have a branch name"),
        );

        assert_eq!(selected.dispatch_id, "dispatch-2");
        assert_eq!(
            pull_request_url.as_deref(),
            Some("https://github.com/acme/project-a/pull/42")
        );
    }

    #[test]
    fn selects_the_latest_previous_submitted_review_run() {
        let review = sample_review_record();
        let dispatch_history = vec![
            ReviewRunRecord {
                dispatch_id: "dispatch-3".to_owned(),
                review_id: review.id.clone(),
                pull_request_url: review.pull_request_url.clone(),
                repository_full_name: review.repository_full_name.clone(),
                workspace_key: review.workspace_key.clone(),
                preferred_tool: review.preferred_tool,
                status: DispatchStatus::Preparing,
                created_at: now_utc(),
                updated_at: now_utc(),
                finished_at: None,
                remote_host: "192.0.2.25".to_owned(),
                branch_name: Some("track-review/dispatch-3".to_owned()),
                worktree_path: Some("~/workspace/project-x/review-worktrees/dispatch-3".to_owned()),
                follow_up_request: Some("Re-review the latest fixes.".to_owned()),
                target_head_oid: Some("ccc333".to_owned()),
                summary: Some("Re-review request: Re-review the latest fixes.".to_owned()),
                review_submitted: false,
                github_review_id: None,
                github_review_url: None,
                notes: None,
                error_message: None,
            },
            ReviewRunRecord {
                dispatch_id: "dispatch-2".to_owned(),
                review_id: review.id.clone(),
                pull_request_url: review.pull_request_url.clone(),
                repository_full_name: review.repository_full_name.clone(),
                workspace_key: review.workspace_key.clone(),
                preferred_tool: review.preferred_tool,
                status: DispatchStatus::Succeeded,
                created_at: now_utc(),
                updated_at: now_utc(),
                finished_at: Some(now_utc()),
                remote_host: "192.0.2.25".to_owned(),
                branch_name: Some("track-review/dispatch-2".to_owned()),
                worktree_path: Some("~/workspace/project-x/review-worktrees/dispatch-2".to_owned()),
                follow_up_request: None,
                target_head_oid: Some("bbb222".to_owned()),
                summary: Some("Submitted a review.".to_owned()),
                review_submitted: true,
                github_review_id: Some("1002".to_owned()),
                github_review_url: Some(
                    "https://github.com/acme/project-x/pull/42#pullrequestreview-1002".to_owned(),
                ),
                notes: None,
                error_message: None,
            },
            ReviewRunRecord {
                dispatch_id: "dispatch-1".to_owned(),
                review_id: review.id.clone(),
                pull_request_url: review.pull_request_url.clone(),
                repository_full_name: review.repository_full_name.clone(),
                workspace_key: review.workspace_key.clone(),
                preferred_tool: review.preferred_tool,
                status: DispatchStatus::Succeeded,
                created_at: now_utc(),
                updated_at: now_utc(),
                finished_at: Some(now_utc()),
                remote_host: "192.0.2.25".to_owned(),
                branch_name: Some("track-review/dispatch-1".to_owned()),
                worktree_path: Some("~/workspace/project-x/review-worktrees/dispatch-1".to_owned()),
                follow_up_request: None,
                target_head_oid: Some("aaa111".to_owned()),
                summary: Some("Submitted an older review.".to_owned()),
                review_submitted: true,
                github_review_id: Some("1001".to_owned()),
                github_review_url: Some(
                    "https://github.com/acme/project-x/pull/42#pullrequestreview-1001".to_owned(),
                ),
                notes: None,
                error_message: None,
            },
        ];

        let selected = select_previous_submitted_review_run(&dispatch_history, "dispatch-3")
            .expect("a previous submitted review should be selected");

        assert_eq!(selected.dispatch_id, "dispatch-2");
        assert_eq!(selected.github_review_id.as_deref(), Some("1002"));
    }

    #[test]
    fn closing_task_stays_local_when_remote_cleanup_is_unavailable() {
        let context = TestContext::new(base_test_config(None));
        let task = context.create_task("project-a", "Investigate a flaky remote cleanup");
        let existing_dispatch = context.create_running_dispatch(&task);

        let updated_task = context
            .service()
            .update_task(
                &task.id,
                TaskUpdateInput {
                    status: Some(Status::Closed),
                    ..TaskUpdateInput::default()
                },
            )
            .expect("closing should still succeed locally");

        let updated_dispatch = context
            .dispatch_repository
            .get_dispatch(&task.id, &existing_dispatch.dispatch_id)
            .expect("dispatch lookup should succeed")
            .expect("dispatch should still exist");

        assert_eq!(updated_task.status, Status::Closed);
        assert_eq!(updated_dispatch.status, DispatchStatus::Canceled);
        assert_eq!(
            updated_dispatch.summary.as_deref(),
            Some("Canceled because the task was closed locally. Remote cleanup was skipped.")
        );
        assert!(updated_dispatch
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("remote-agent configuration is missing")));
    }

    #[test]
    fn deleting_task_stays_local_when_remote_cleanup_is_unavailable() {
        let context = TestContext::new(base_test_config(None));
        let task = context.create_task("project-a", "Delete the task even without remote cleanup");
        let _existing_dispatch = context.create_running_dispatch(&task);

        context
            .service()
            .delete_task(&task.id)
            .expect("delete should still succeed locally");

        let task_error = context
            .task_repository
            .get_task(&task.id)
            .expect_err("deleted task should be gone");
        assert_eq!(task_error.code, crate::errors::ErrorCode::TaskNotFound);
        assert!(context
            .dispatch_repository
            .dispatches_for_task(&task.id)
            .expect("dispatch lookup should succeed")
            .is_empty());
    }

    #[test]
    fn refresh_releases_active_dispatches_when_remote_config_disappears() {
        let context = TestContext::new(base_test_config(None));
        let task = context.create_task("project-a", "Recover from a missing remote config");
        let existing_dispatch = context.create_running_dispatch(&task);

        let refreshed = context
            .service()
            .latest_dispatches_for_tasks(std::slice::from_ref(&task.id))
            .expect("dispatch refresh should succeed");
        let updated_dispatch = refreshed
            .first()
            .expect("latest dispatch should still be returned");

        assert_eq!(updated_dispatch.dispatch_id, existing_dispatch.dispatch_id);
        assert_eq!(updated_dispatch.status, DispatchStatus::Blocked);
        assert_eq!(
            updated_dispatch.summary.as_deref(),
            Some("Remote reconciliation is unavailable locally, so active runs were released.")
        );
        assert_eq!(
            updated_dispatch.error_message.as_deref(),
            Some("Remote agent configuration is missing locally.")
        );
    }

    #[test]
    fn queue_dispatch_releases_stale_active_dispatch_when_remote_refresh_fails() {
        let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
            host: "127.0.0.1".to_owned(),
            user: "builder".to_owned(),
            port: 1,
            workspace_root: "~/workspace".to_owned(),
            projects_registry_path: "~/track-projects.json".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
            review_follow_up: None,
        })));
        let task =
            context.create_task("project-a", "Retry after the previous remote run got stuck");
        context.write_project_metadata(&task.project);
        let existing_dispatch = context.create_running_dispatch(&task);

        let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
        install_dummy_managed_remote_agent_material(&context.data_dir);

        let queued_dispatch = context
            .service()
            .queue_dispatch(&task.id, None)
            .expect("queueing should release the stale active dispatch first");
        let released_dispatch = context
            .dispatch_repository
            .get_dispatch(&task.id, &existing_dispatch.dispatch_id)
            .expect("dispatch lookup should succeed")
            .expect("previous dispatch should still exist");

        assert_ne!(queued_dispatch.dispatch_id, existing_dispatch.dispatch_id);
        assert_eq!(queued_dispatch.status, DispatchStatus::Preparing);
        assert_eq!(released_dispatch.status, DispatchStatus::Blocked);
        assert_eq!(
            released_dispatch.summary.as_deref(),
            Some(
                "Remote reconciliation could not reach the remote machine, so active runs were released locally."
            )
        );
        assert!(released_dispatch.error_message.is_some());
    }

    #[test]
    fn follow_up_dispatch_keeps_the_original_runner_tool() {
        let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
            host: "127.0.0.1".to_owned(),
            user: "builder".to_owned(),
            port: 2222,
            workspace_root: "~/workspace".to_owned(),
            projects_registry_path: "~/track-projects.json".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
            review_follow_up: None,
        })));
        let task = context.create_task("project-a", "Keep using the same runner on follow-up");
        context.write_project_metadata(&task.project);
        let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
        install_dummy_managed_remote_agent_material(&context.data_dir);

        let mut first_dispatch = context
            .service()
            .queue_dispatch(&task.id, Some(RemoteAgentPreferredTool::Claude))
            .expect("initial dispatch should queue");
        first_dispatch.status = DispatchStatus::Succeeded;
        first_dispatch.finished_at = Some(first_dispatch.updated_at);
        context
            .dispatch_repository
            .save_dispatch(&first_dispatch)
            .expect("initial dispatch should save as terminal");

        let follow_up_dispatch = context
            .service()
            .queue_follow_up_dispatch(&task.id, "Address the review comments.")
            .expect("follow-up dispatch should queue");

        assert_eq!(
            first_dispatch.preferred_tool,
            RemoteAgentPreferredTool::Claude
        );
        assert_eq!(
            follow_up_dispatch.preferred_tool,
            RemoteAgentPreferredTool::Claude
        );
    }

    #[test]
    fn reset_blockers_include_active_review_runs() {
        let context = TestContext::new(base_test_config(None));
        let task = context.create_task("project-a", "Keep reset from interrupting live work");
        let task_dispatch = context.create_running_dispatch(&task);
        let review = context.create_review();
        let review_dispatch = context.create_review_run(&review, DispatchStatus::Running);

        let blockers = describe_remote_reset_blockers(&[task_dispatch], &[review_dispatch]);

        assert_eq!(blockers.len(), 2);
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains(&task.id) && blocker.contains("task")));
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains(&review.id) && blocker.contains("review")));
    }

    #[test]
    fn task_dispatch_start_guard_serializes_same_task() {
        let acquired_in_second_thread = Arc::new(AtomicBool::new(false));
        let guard = super::TaskDispatchStartGuard::acquire("task-1");

        std::thread::scope(|scope| {
            let acquired_in_second_thread_for_join = Arc::clone(&acquired_in_second_thread);
            let join_handle = scope.spawn(move || {
                let _guard = super::TaskDispatchStartGuard::acquire("task-1");
                acquired_in_second_thread_for_join.store(true, Ordering::SeqCst);
            });

            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(
                !acquired_in_second_thread.load(Ordering::SeqCst),
                "the second same-task start should stay blocked while the first guard is held",
            );

            drop(guard);
            join_handle
                .join()
                .expect("second thread should acquire the guard after release");
        });

        assert!(
            acquired_in_second_thread.load(Ordering::SeqCst),
            "the waiting same-task start should proceed after the first guard releases",
        );
    }

    #[test]
    fn review_dispatch_start_guard_serializes_same_review() {
        let acquired_in_second_thread = Arc::new(AtomicBool::new(false));
        let guard = super::ReviewDispatchStartGuard::acquire("review-1");

        std::thread::scope(|scope| {
            let acquired_in_second_thread_for_join = Arc::clone(&acquired_in_second_thread);
            let join_handle = scope.spawn(move || {
                let _guard = super::ReviewDispatchStartGuard::acquire("review-1");
                acquired_in_second_thread_for_join.store(true, Ordering::SeqCst);
            });

            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(
                !acquired_in_second_thread.load(Ordering::SeqCst),
                "the second same-review start should stay blocked while the first guard is held",
            );

            drop(guard);
            join_handle
                .join()
                .expect("second thread should acquire the guard after release");
        });

        assert!(
            acquired_in_second_thread.load(Ordering::SeqCst),
            "the waiting same-review start should proceed after the first guard releases",
        );
    }
}
