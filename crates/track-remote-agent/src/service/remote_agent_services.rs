use std::collections::BTreeSet;

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::remote_layout::{
    DispatchBranch, DispatchRunDirectory, DispatchWorktreePath, RemoteCheckoutPath, WorkspaceKey,
};
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{
    DispatchStatus, RemoteAgentPreferredTool, RemoteCleanupSummary, RemoteResetSummary,
    ReviewRecord, Status, TaskDispatchRecord,
};

use crate::prompts::RemoteDispatchPrompt;
use crate::remote_actions::{
    CancelRemoteDispatchAction, CleanupOrphanedRemoteArtifactsAction, CleanupReviewArtifactsAction,
    CleanupReviewWorkspaceCachesAction, CleanupTaskArtifactsAction, CreateReviewWorktreeAction,
    CreateWorktreeAction, EnsureCheckoutAction, EnsureFollowUpWorktreeAction,
    FetchGithubLoginAction, FetchPullRequestReviewStateAction, LaunchRemoteDispatchAction,
    LoadRemoteRegistryAction, PostPullRequestCommentAction, ReadDispatchSnapshotsAction,
    ResetWorkspaceAction, UploadRemoteFileAction, WriteRemoteRegistryAction,
};
use crate::ssh::SshClient;
use crate::types::{
    RemoteArtifactCleanupCounts, RemoteDispatchSnapshot, RemoteProjectRegistryEntry,
    RemoteProjectRegistryFile, RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation,
    RemoteTaskCleanupMode,
};
use crate::utils::{
    build_review_follow_up_notification_comment, contextualize_track_error,
    describe_remote_reset_blockers, parse_github_repository_name, unique_review_run_directories,
    unique_review_worktree_paths,
};

use super::dispatch::{unique_run_directories, unique_worktree_paths, RemoteDispatchService};
use super::review::RemoteReviewService;

#[async_trait::async_trait]
pub trait RemoteAgentConfigProvider: Send + Sync {
    async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError>;
}

#[async_trait::async_trait]
impl<T: RemoteAgentConfigProvider + ?Sized> RemoteAgentConfigProvider for std::sync::Arc<T> {
    async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        (**self).load_remote_agent_runtime_config().await
    }
}

pub struct RemoteAgentServices<'a> {
    config_service: &'a dyn RemoteAgentConfigProvider,
    database: &'a DatabaseContext,
}

impl<'a> RemoteAgentServices<'a> {
    pub fn new(
        config_service: &'a dyn RemoteAgentConfigProvider,
        database: &'a DatabaseContext,
    ) -> Self {
        Self {
            config_service,
            database,
        }
    }

    fn dispatch_repository(&self) -> DispatchRepository<'a> {
        self.database.dispatch_repository()
    }

    fn project_repository(&self) -> ProjectRepository<'a> {
        self.database.project_repository()
    }

    fn task_repository(&self) -> FileTaskRepository<'a> {
        self.database.task_repository()
    }

    fn review_repository(&self) -> ReviewRepository<'a> {
        self.database.review_repository()
    }

    fn review_dispatch_repository(&self) -> ReviewDispatchRepository<'a> {
        self.database.review_dispatch_repository()
    }

    pub fn dispatch(&self) -> RemoteDispatchService<'a> {
        RemoteDispatchService {
            config_service: self.config_service,
            database: self.database,
        }
    }

    pub fn review(&self) -> RemoteReviewService<'a> {
        RemoteReviewService {
            config_service: self.config_service,
            database: self.database,
        }
    }

    // =============================================================================
    // Manual Remote Cleanup
    // =============================================================================
    //
    // Dispatch and review runs are tracked separately, but the user-facing
    // cleanup command has to reconcile both domains against one shared remote
    // workspace. The top-level service owner handles that coordination so the
    // dispatch and review application services can stay focused on their own
    // local records and workflows.
    pub async fn cleanup_unused_remote_artifacts(
        &self,
    ) -> Result<RemoteCleanupSummary, TrackError> {
        let dispatch_service = self.dispatch();
        let remote_agent = self.load_remote_agent_for_global_cleanup().await?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
        let task_ids_with_history = self.dispatch_repository().task_ids_with_history().await?;
        let review_ids_with_history = self
            .review_dispatch_repository()
            .review_ids_with_history()
            .await?;
        let tracked_project_names = self
            .project_repository()
            .list_projects()
            .await?
            .into_iter()
            .map(|project| project.canonical_name.as_workspace_key())
            .collect::<BTreeSet<_>>();

        let mut summary = RemoteCleanupSummary::default();
        let mut kept_worktree_paths = BTreeSet::new();
        let mut kept_run_directories = BTreeSet::new();
        let mut review_workspace_keys = BTreeSet::new();
        let mut active_review_workspace_keys = BTreeSet::new();

        for task_id in task_ids_with_history {
            let dispatch_history = self
                .dispatch_repository()
                .dispatches_for_task(&task_id)
                .await?;
            if dispatch_history.is_empty() {
                continue;
            }

            match self.task_repository().get_task(&task_id).await {
                Ok(task) if task.status == Status::Open => {
                    kept_worktree_paths.extend(unique_worktree_paths(&dispatch_history));
                    kept_run_directories
                        .extend(unique_run_directories(&dispatch_history, &remote_agent));
                }
                Ok(task) if task.status == Status::Closed => {
                    let cleanup_counts = dispatch_service
                        .cleanup_task_remote_artifacts(
                            &task.id,
                            &dispatch_history,
                            RemoteTaskCleanupMode::CloseTask,
                        )
                        .await?;
                    dispatch_service
                        .finalize_active_dispatches_locally(
                            &dispatch_history,
                            DispatchStatus::Canceled,
                            "Canceled because the task was closed.",
                            None,
                        )
                        .await?;
                    kept_run_directories
                        .extend(unique_run_directories(&dispatch_history, &remote_agent));
                    summary.closed_tasks_cleaned += 1;
                    summary.remote_worktrees_removed += cleanup_counts.worktrees_removed;
                    summary.remote_run_directories_removed +=
                        cleanup_counts.run_directories_removed;
                }
                Err(error) if error.code == ErrorCode::TaskNotFound => {
                    let cleanup_counts = dispatch_service
                        .cleanup_task_remote_artifacts(
                            &task_id,
                            &dispatch_history,
                            RemoteTaskCleanupMode::DeleteTask,
                        )
                        .await?;
                    self.dispatch_repository()
                        .delete_dispatch_history_for_task(&task_id)
                        .await?;
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
                .review_dispatch_repository()
                .dispatches_for_review(&review_id)
                .await?;
            if dispatch_history.is_empty() {
                continue;
            }

            let workspace_key = dispatch_history[0].workspace_key.clone();
            review_workspace_keys.insert(workspace_key.clone());

            match self.review_repository().get_review(&review_id).await {
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
                    self.review_dispatch_repository()
                        .delete_dispatch_history_for_review(&review_id)
                        .await?;
                    summary.local_dispatch_histories_removed += 1;
                }
                Err(error) => return Err(error),
            }
        }

        let orphan_cleanup_counts = workspace.cleanup_orphaned_artifacts(
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
        workspace.cleanup_reclaimable_review_workspaces(&reclaimable_review_workspace_keys)?;

        Ok(summary)
    }

    // =============================================================================
    // Full Remote Workspace Reset
    // =============================================================================
    //
    // Reset is also cross-domain by definition: we need both task dispatches
    // and review runs to be idle before we drop the shared remote workspace.
    pub async fn reset_remote_workspace(&self) -> Result<RemoteResetSummary, TrackError> {
        let active_task_dispatches = self.dispatch().list_dispatches(None).await?;
        let active_review_dispatches = self.review().list_dispatches(None).await?;
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

        let remote_agent = self.load_remote_agent_for_global_cleanup().await?;
        let ssh_client = SshClient::new(&remote_agent)?;
        RemoteWorkspaceOps::new(&ssh_client, &remote_agent).reset_workspace()
    }

    pub async fn reconcile_review_follow_up(
        &self,
    ) -> Result<RemoteReviewFollowUpReconciliation, TrackError> {
        let remote_agent = match self.config_service.load_remote_agent_runtime_config().await {
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

        let task_ids = self.dispatch_repository().task_ids_with_history().await?;
        if task_ids.is_empty() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let dispatch_service = self.dispatch();
        let latest_dispatches = dispatch_service
            .latest_dispatches_for_tasks(&task_ids)
            .await?;
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

            match self
                .task_repository()
                .get_task(&dispatch_record.task_id)
                .await
            {
                Ok(task) if task.status == Status::Open => task,
                Ok(_) => continue,
                Err(error) if error.code == ErrorCode::TaskNotFound => continue,
                Err(error) => return Err(error),
            };

            let pull_request_state = FetchPullRequestReviewStateAction::new(
                &ssh_client,
                pull_request_url,
                &review_follow_up.main_user,
            )
            .execute()
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
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "fetch_failed",
                        error.to_string(),
                        &dispatch_record,
                        &review_follow_up.main_user,
                        None,
                    ));
                    continue;
                }
            };

            reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                "task_evaluated",
                "Fetched PR review state for automatic follow-up reconciliation.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            if !pull_request_state.is_open {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "skip_closed_pr",
                    "Skipped automatic follow-up because the PR is not open anymore.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            if dispatch_record.status.is_active() {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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
                    let follow_up_request = RemoteDispatchPrompt::build_review_follow_up_request(
                        pull_request_url,
                        &review_follow_up.main_user,
                        dispatch_record.created_at,
                    );
                    let queued_dispatch = dispatch_service
                        .queue_follow_up_dispatch(&dispatch_record.task_id, &follow_up_request)
                        .await?;
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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
            let notify_reviewer_result = PostPullRequestCommentAction::new(
                &ssh_client,
                pull_request_url,
                &notification_comment,
            )
            .execute()
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
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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
            )
            .await?;
            reconciliation.events.push(RemoteReviewFollowUpEvent::new(
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

    async fn load_remote_agent_for_global_cleanup(
        &self,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()
            .await?
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

    async fn mark_review_notification_for_head(
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
        self.dispatch_repository()
            .save_dispatch(&updated_record)
            .await
    }
}

// =============================================================================
// Shared Remote Runner Operations
// =============================================================================
//
// Dispatches and reviews share the same SSH-backed mechanics for uploading
// prompt files, launching runs, canceling them, and reading snapshots. These
// helpers stay in the owner module because they are infrastructure, not either
// domain's business policy.
pub(super) struct RemoteRunOps<'a> {
    ssh_client: &'a SshClient,
}

impl<'a> RemoteRunOps<'a> {
    pub(super) fn new(ssh_client: &'a SshClient) -> Self {
        Self { ssh_client }
    }

    pub(super) fn upload_prompt_and_schema(
        &self,
        prompt_path: &str,
        prompt: &str,
        schema_path: &str,
        schema: &str,
    ) -> Result<(), TrackError> {
        UploadRemoteFileAction::new(self.ssh_client, prompt_path, prompt).execute()?;
        UploadRemoteFileAction::new(self.ssh_client, schema_path, schema).execute()?;
        Ok(())
    }

    pub(super) fn launch(
        &self,
        remote_run_directory: &DispatchRunDirectory,
        worktree_path: &DispatchWorktreePath,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Result<(), TrackError> {
        let remote_run_directory = remote_run_directory.clone().into_inner();
        let worktree_path = worktree_path.clone().into_inner();
        LaunchRemoteDispatchAction::new(
            self.ssh_client,
            &remote_run_directory,
            &worktree_path,
            preferred_tool,
        )
        .execute()
    }

    pub(super) fn cancel(
        &self,
        remote_run_directory: &DispatchRunDirectory,
    ) -> Result<(), TrackError> {
        let remote_run_directory = remote_run_directory.clone().into_inner();
        CancelRemoteDispatchAction::new(self.ssh_client, &remote_run_directory).execute()
    }

    pub(super) fn read_snapshots(
        &self,
        run_directories: &[String],
    ) -> Result<Vec<RemoteDispatchSnapshot>, TrackError> {
        ReadDispatchSnapshotsAction::new(self.ssh_client, run_directories).execute()
    }
}

// =============================================================================
// Shared Remote Workspace Operations
// =============================================================================
//
// The remote registry and checkout layout are shared across task dispatches,
// PR reviews, cleanup, and reset. Keeping that machinery here prevents the
// domain services from mixing persistence concerns with shell-script details.
pub(super) struct RemoteWorkspaceOps<'a> {
    ssh_client: &'a SshClient,
    remote_agent: &'a RemoteAgentRuntimeConfig,
}

impl<'a> RemoteWorkspaceOps<'a> {
    pub(super) fn new(
        ssh_client: &'a SshClient,
        remote_agent: &'a RemoteAgentRuntimeConfig,
    ) -> Self {
        Self {
            ssh_client,
            remote_agent,
        }
    }

    pub(super) fn ensure_task_checkout(
        &self,
        project_name: &track_types::ids::ProjectId,
        metadata: &ProjectMetadata,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let mut remote_registry = self.load_registry()?;
        let github_login = FetchGithubLoginAction::new(self.ssh_client).execute()?;
        let repository_name = parse_github_repository_name(&metadata.repo_url)?;
        let checkout_path = self.checkout_path_from_registry_or_default(
            &remote_registry,
            &project_name.as_workspace_key(),
        );
        let checkout_path_string = checkout_path.clone().into_inner();
        let fork_git_url = EnsureCheckoutAction::new(
            self.ssh_client,
            metadata,
            &repository_name,
            &checkout_path_string,
            &github_login,
        )
        .execute()?;

        remote_registry.projects.insert(
            project_name.as_workspace_key(),
            RemoteProjectRegistryEntry::from_project_metadata(
                checkout_path.clone().into_inner(),
                fork_git_url,
                metadata,
            ),
        );
        self.write_registry(&remote_registry)?;

        Ok(checkout_path)
    }

    pub(super) fn ensure_review_checkout(
        &self,
        review: &ReviewRecord,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let mut remote_registry = self.load_registry()?;
        let github_login = FetchGithubLoginAction::new(self.ssh_client).execute()?;
        let repository_name = parse_github_repository_name(&review.repo_url)?;
        let checkout_path =
            self.checkout_path_from_registry_or_default(&remote_registry, &review.workspace_key);
        let checkout_path_string = checkout_path.clone().into_inner();
        let review_metadata = ProjectMetadata {
            repo_url: review.repo_url.clone(),
            git_url: review.git_url.clone(),
            base_branch: review.base_branch.clone(),
            description: None,
        };
        let fork_git_url = EnsureCheckoutAction::new(
            self.ssh_client,
            &review_metadata,
            &repository_name,
            &checkout_path_string,
            &github_login,
        )
        .execute()?;

        remote_registry.projects.insert(
            review.workspace_key.clone(),
            RemoteProjectRegistryEntry::from_review(
                checkout_path.clone().into_inner(),
                fork_git_url,
                review,
            ),
        );
        self.write_registry(&remote_registry)?;

        Ok(checkout_path)
    }

    pub(super) fn prepare_task_worktree(
        &self,
        checkout_path: &RemoteCheckoutPath,
        base_branch: &str,
        branch_name: &DispatchBranch,
        worktree_path: &DispatchWorktreePath,
        reuse_existing_worktree: bool,
    ) -> Result<(), TrackError> {
        let checkout_path = checkout_path.clone().into_inner();
        let branch_name = branch_name.clone().into_inner();
        let worktree_path = worktree_path.clone().into_inner();
        if reuse_existing_worktree {
            EnsureFollowUpWorktreeAction::new(
                self.ssh_client,
                &checkout_path,
                &branch_name,
                &worktree_path,
            )
            .execute()
        } else {
            CreateWorktreeAction::new(
                self.ssh_client,
                &checkout_path,
                base_branch,
                &branch_name,
                &worktree_path,
            )
            .execute()
        }
    }

    pub(super) fn prepare_review_worktree(
        &self,
        checkout_path: &RemoteCheckoutPath,
        pull_request_number: u64,
        branch_name: &DispatchBranch,
        worktree_path: &DispatchWorktreePath,
        target_head_oid: Option<&str>,
    ) -> Result<(), TrackError> {
        let checkout_path = checkout_path.clone().into_inner();
        let branch_name = branch_name.clone().into_inner();
        let worktree_path = worktree_path.clone().into_inner();
        CreateReviewWorktreeAction::new(
            self.ssh_client,
            &checkout_path,
            pull_request_number,
            &branch_name,
            &worktree_path,
            target_head_oid,
        )
        .execute()
    }

    pub(super) fn resolve_checkout_path(
        &self,
        workspace_key: &WorkspaceKey,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let remote_registry = self.load_registry()?;
        Ok(self.checkout_path_from_registry_or_default(&remote_registry, workspace_key))
    }

    pub(super) fn cleanup_task_artifacts(
        &self,
        checkout_path: &RemoteCheckoutPath,
        worktree_paths: &[String],
        run_directories: &[String],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let checkout_path = checkout_path.clone().into_inner();
        CleanupTaskArtifactsAction::new(
            self.ssh_client,
            &checkout_path,
            worktree_paths,
            run_directories,
            cleanup_mode,
        )
        .execute()
    }

    pub(super) fn cleanup_review_artifacts(
        &self,
        checkout_path: &RemoteCheckoutPath,
        branch_names: &[String],
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Result<(), TrackError> {
        let checkout_path = checkout_path.clone().into_inner();
        CleanupReviewArtifactsAction::new(
            self.ssh_client,
            &checkout_path,
            branch_names,
            worktree_paths,
            run_directories,
        )
        .execute()
    }

    pub(super) fn cleanup_orphaned_artifacts(
        &self,
        kept_worktree_paths: &[String],
        kept_run_directories: &[String],
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        CleanupOrphanedRemoteArtifactsAction::new(
            self.ssh_client,
            &self.remote_agent.workspace_root,
            kept_worktree_paths,
            kept_run_directories,
        )
        .execute()
    }

    pub(super) fn cleanup_reclaimable_review_workspaces(
        &self,
        workspace_keys: &[WorkspaceKey],
    ) -> Result<(), TrackError> {
        if workspace_keys.is_empty() {
            return Ok(());
        }

        let mut remote_registry = self.load_registry()?;
        let checkout_paths = workspace_keys
            .iter()
            .map(|workspace_key| {
                self.checkout_path_from_registry_or_default(&remote_registry, workspace_key)
                    .into_inner()
            })
            .collect::<Vec<_>>();

        CleanupReviewWorkspaceCachesAction::new(self.ssh_client, &checkout_paths).execute()?;

        let mut registry_changed = false;
        for workspace_key in workspace_keys {
            registry_changed |= remote_registry.projects.remove(workspace_key).is_some();
        }

        if registry_changed {
            self.write_registry(&remote_registry)?;
        }

        Ok(())
    }

    pub(super) fn reset_workspace(&self) -> Result<RemoteResetSummary, TrackError> {
        ResetWorkspaceAction::new(
            self.ssh_client,
            &self.remote_agent.workspace_root,
            &self.remote_agent.projects_registry_path,
        )
        .execute()
    }

    fn load_registry(&self) -> Result<RemoteProjectRegistryFile, TrackError> {
        LoadRemoteRegistryAction::new(self.ssh_client, &self.remote_agent.projects_registry_path)
            .execute()
    }

    fn write_registry(
        &self,
        remote_registry: &RemoteProjectRegistryFile,
    ) -> Result<(), TrackError> {
        WriteRemoteRegistryAction::new(
            self.ssh_client,
            &self.remote_agent.projects_registry_path,
            remote_registry,
        )
        .execute()
    }

    fn checkout_path_from_registry_or_default(
        &self,
        remote_registry: &RemoteProjectRegistryFile,
        workspace_key: &WorkspaceKey,
    ) -> RemoteCheckoutPath {
        remote_registry
            .projects
            .get(workspace_key)
            .map(|entry| RemoteCheckoutPath::from_registry_unchecked(entry.checkout_path.clone()))
            .unwrap_or_else(|| self.default_checkout_path(workspace_key))
    }

    fn default_checkout_path(&self, workspace_key: &WorkspaceKey) -> RemoteCheckoutPath {
        RemoteCheckoutPath::for_workspace(&self.remote_agent.workspace_root, workspace_key)
    }
}

pub(super) enum RefreshRemoteClient {
    Available(SshClient),
    UnavailableLocally { error_message: String },
}

pub(super) async fn load_refresh_ssh_client(
    config_service: &dyn RemoteAgentConfigProvider,
) -> Result<RefreshRemoteClient, TrackError> {
    let remote_agent = match config_service.load_remote_agent_runtime_config().await {
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
