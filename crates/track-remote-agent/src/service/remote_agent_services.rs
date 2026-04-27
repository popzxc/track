use std::collections::BTreeSet;

use track_config::paths::collapse_home_path;
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{
    DispatchStatus, RemoteCleanupSummary, RemoteResetSummary, Status, TaskDispatchRecord,
};

use crate::prompts::RemoteDispatchPrompt;
use crate::types::{
    RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation, RemoteTaskCleanupMode,
};
use crate::utils::{
    build_review_follow_up_notification_comment, contextualize_track_error,
    describe_remote_reset_blockers, unique_review_run_directories, unique_review_worktree_paths,
};
use crate::{invalidate_helper_upload, RemoteWorkspace};

use super::dispatch::{unique_run_directories, unique_worktree_paths, RemoteDispatchService};
use super::review::RemoteReviewService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewNotificationDecision {
    SkipMissingHeadOid,
    SkipAlreadyRecorded,
    Post,
}

fn review_notification_decision(
    dispatch_record: &TaskDispatchRecord,
    head_oid: &str,
    review_user: &str,
) -> ReviewNotificationDecision {
    let trimmed_head_oid = head_oid.trim();
    if trimmed_head_oid.is_empty() {
        return ReviewNotificationDecision::SkipMissingHeadOid;
    }

    let already_recorded_for_head = dispatch_record.review_request_head_oid.as_deref()
        == Some(trimmed_head_oid)
        && dispatch_record.review_request_user.as_deref() == Some(review_user);
    if already_recorded_for_head {
        ReviewNotificationDecision::SkipAlreadyRecorded
    } else {
        ReviewNotificationDecision::Post
    }
}

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
    #[tracing::instrument(skip(self))]
    pub async fn cleanup_unused_remote_artifacts(
        &self,
    ) -> Result<RemoteCleanupSummary, TrackError> {
        let dispatch_service = self.dispatch();
        let remote_agent = self.load_remote_agent_for_global_cleanup().await?;
        let workspace = self.remote_workspace(remote_agent.clone())?;
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
                        .filter(|record| record.run.status.is_active())
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

        let orphan_cleanup_counts = workspace
            .maintenance()
            .cleanup_orphaned_artifacts(
                &kept_worktree_paths.into_iter().collect::<Vec<_>>(),
                &kept_run_directories.into_iter().collect::<Vec<_>>(),
            )
            .await?;
        summary.remote_worktrees_removed += orphan_cleanup_counts.worktrees_removed;
        summary.remote_run_directories_removed += orphan_cleanup_counts.run_directories_removed;

        let reclaimable_review_workspace_keys = review_workspace_keys
            .into_iter()
            .filter(|workspace_key| {
                !tracked_project_names.contains(workspace_key)
                    && !active_review_workspace_keys.contains(workspace_key)
            })
            .collect::<Vec<_>>();
        workspace
            .maintenance()
            .cleanup_reclaimable_review_workspaces(&reclaimable_review_workspace_keys)
            .await?;

        tracing::info!(
            closed_tasks_cleaned = summary.closed_tasks_cleaned,
            missing_tasks_cleaned = summary.missing_tasks_cleaned,
            local_dispatch_histories_removed = summary.local_dispatch_histories_removed,
            remote_worktrees_removed = summary.remote_worktrees_removed,
            remote_run_directories_removed = summary.remote_run_directories_removed,
            reclaimable_review_workspaces = reclaimable_review_workspace_keys.len(),
            "Completed remote artifact cleanup"
        );

        Ok(summary)
    }

    // =============================================================================
    // Full Remote Workspace Reset
    // =============================================================================
    //
    // Reset is also cross-domain by definition: we need both task dispatches
    // and review runs to be idle before we drop the shared remote workspace.
    #[tracing::instrument(skip(self))]
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
        let summary = self
            .remote_workspace(remote_agent)?
            .maintenance()
            .reset_workspace()
            .await?;
        invalidate_helper_upload();
        tracing::warn!(
            workspace_entries_removed = summary.workspace_entries_removed,
            registry_removed = summary.registry_removed,
            "Reset remote workspace"
        );
        Ok(summary)
    }

    #[tracing::instrument(skip(self))]
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
        let tasks_by_id = self
            .task_repository()
            .tasks_by_ids(
                &latest_dispatches
                    .iter()
                    .map(|dispatch_record| dispatch_record.task_id.clone())
                    .collect::<Vec<_>>(),
            )
            .await?
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect::<std::collections::BTreeMap<_, _>>();
        let workspace = self.remote_workspace(remote_agent)?;
        let mut reconciliation = RemoteReviewFollowUpReconciliation::default();

        for dispatch_record in latest_dispatches {
            let Some(pull_request_url) = dispatch_record.pull_request_url.as_ref() else {
                continue;
            };

            let Some(task) = tasks_by_id.get(&dispatch_record.task_id) else {
                continue;
            };
            if task.status != Status::Open {
                continue;
            }

            let pull_request_state = workspace
                .projects()
                .fetch_pull_request_review_state(pull_request_url, &review_follow_up.main_user)
                .await
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
                    tracing::warn!(
                        task_id = %dispatch_record.task_id,
                        dispatch_id = %dispatch_record.run.dispatch_id,
                        pull_request_url = %pull_request_url,
                        reviewer = %review_follow_up.main_user,
                        error = %error,
                        "Automatic PR follow-up inspection failed"
                    );
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

            if dispatch_record.run.status.is_active() {
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
                if latest_review.submitted_at > dispatch_record.run.created_at {
                    let follow_up_request = RemoteDispatchPrompt::build_review_follow_up_request(
                        pull_request_url,
                        &review_follow_up.main_user,
                        dispatch_record.run.created_at,
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
                            dispatch_record.run.dispatch_id,
                        ),
                        &queued_dispatch,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    tracing::info!(
                        task_id = %queued_dispatch.task_id,
                        dispatch_id = %queued_dispatch.run.dispatch_id,
                        reviewer = %review_follow_up.main_user,
                        pull_request_url = %pull_request_url,
                        "Queued automatic follow-up dispatch from PR review state"
                    );
                    self.mark_review_notification_for_head(
                        &queued_dispatch,
                        &pull_request_state.head_oid,
                        &review_follow_up.main_user,
                    )
                    .await?;
                    reconciliation.queued_dispatches.push(queued_dispatch);
                    continue;
                }
            }

            let head_oid = pull_request_state.head_oid.trim();
            match review_notification_decision(
                &dispatch_record,
                head_oid,
                &review_follow_up.main_user,
            ) {
                ReviewNotificationDecision::SkipMissingHeadOid => {
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "skip_missing_head_oid",
                        "Skipped PR reviewer notification because the PR head SHA is missing.",
                        &dispatch_record,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    continue;
                }
                ReviewNotificationDecision::SkipAlreadyRecorded => {
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "skip_notification_already_recorded",
                        "Skipped PR reviewer notification because this PR head already recorded the same reviewer.",
                        &dispatch_record,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    continue;
                }
                ReviewNotificationDecision::Post => {}
            }

            let notification_comment =
                build_review_follow_up_notification_comment(&review_follow_up.main_user, head_oid);
            let notify_reviewer_result = workspace
                .projects()
                .post_pull_request_comment(pull_request_url, &notification_comment)
                .await
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
                tracing::warn!(
                    task_id = %dispatch_record.task_id,
                    dispatch_id = %dispatch_record.run.dispatch_id,
                    reviewer = %review_follow_up.main_user,
                    pull_request_url = %pull_request_url,
                    error = %error,
                    "Posting PR reviewer notification failed"
                );
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
                head_oid,
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
            tracing::info!(
                task_id = %dispatch_record.task_id,
                dispatch_id = %dispatch_record.run.dispatch_id,
                reviewer = %review_follow_up.main_user,
                pull_request_url = %pull_request_url,
                "Posted PR reviewer follow-up notification"
            );
        }

        tracing::info!(
            review_notifications_updated = reconciliation.review_notifications_updated,
            queued_dispatches = reconciliation.queued_dispatches.len(),
            failures = reconciliation.failures,
            evaluated_events = reconciliation.events.len(),
            "Completed review follow-up reconciliation"
        );

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

    fn remote_workspace(
        &self,
        remote_agent: RemoteAgentRuntimeConfig,
    ) -> Result<RemoteWorkspace, TrackError> {
        RemoteWorkspace::new(remote_agent, self.database.clone())
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

pub(super) enum RefreshRemoteWorkspace {
    Available(Box<RemoteWorkspace>),
    UnavailableLocally { error_message: String },
}

pub(super) async fn load_refresh_remote_workspace(
    config_service: &dyn RemoteAgentConfigProvider,
    database: &DatabaseContext,
) -> Result<RefreshRemoteWorkspace, TrackError> {
    let remote_agent = match config_service.load_remote_agent_runtime_config().await {
        Ok(Some(config)) => config,
        Ok(None) => {
            return Ok(RefreshRemoteWorkspace::UnavailableLocally {
                error_message: "Remote agent configuration is missing locally.".to_owned(),
            });
        }
        Err(error) if error.remote_unavailable() => {
            return Ok(RefreshRemoteWorkspace::UnavailableLocally {
                error_message: error.to_string(),
            });
        }
        Err(error) => return Err(error),
    };

    if !remote_agent.managed_key_path.exists() {
        return Ok(RefreshRemoteWorkspace::UnavailableLocally {
            error_message: format!(
                "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                collapse_home_path(&remote_agent.managed_key_path)
            ),
        });
    }

    Ok(RefreshRemoteWorkspace::Available(Box::new(
        RemoteWorkspace::new(remote_agent, database.clone())?,
    )))
}

#[cfg(test)]
mod tests {
    use super::{review_notification_decision, ReviewNotificationDecision};
    use track_types::ids::{DispatchId, ProjectId, TaskId};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, RemoteRunState, TaskDispatchRecord,
    };

    fn sample_dispatch_record() -> TaskDispatchRecord {
        let dispatch_id = DispatchId::new("dispatch-1").expect("dispatch ids should parse");
        let timestamp = now_utc();
        TaskDispatchRecord {
            run: RemoteRunState {
                dispatch_id,
                preferred_tool: RemoteAgentPreferredTool::Codex,
                status: DispatchStatus::Succeeded,
                created_at: timestamp,
                updated_at: timestamp,
                finished_at: Some(timestamp),
                remote_host: "127.0.0.1".to_owned(),
                branch_name: None,
                worktree_path: None,
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: None,
            },
            task_id: TaskId::new("task-1").expect("task ids should parse"),
            project: ProjectId::new("project-a").expect("project ids should parse"),
            pull_request_url: None,
            review_request_head_oid: None,
            review_request_user: None,
        }
    }

    #[test]
    fn initial_dispatch_is_notified_when_no_tracking_checkpoint_exists() {
        let dispatch_record = sample_dispatch_record();

        let decision = review_notification_decision(&dispatch_record, "abc123", "octocat");

        assert_eq!(decision, ReviewNotificationDecision::Post);
    }

    #[test]
    fn duplicate_notification_is_skipped_for_same_head_and_user() {
        let mut dispatch_record = sample_dispatch_record();
        dispatch_record.review_request_head_oid = Some("abc123".to_owned());
        dispatch_record.review_request_user = Some("octocat".to_owned());

        let decision = review_notification_decision(&dispatch_record, "abc123", "octocat");

        assert_eq!(decision, ReviewNotificationDecision::SkipAlreadyRecorded);
    }

    #[test]
    fn notification_posts_again_when_head_or_user_changes() {
        let mut dispatch_record = sample_dispatch_record();
        dispatch_record.review_request_head_oid = Some("abc123".to_owned());
        dispatch_record.review_request_user = Some("octocat".to_owned());

        let decision_for_new_head =
            review_notification_decision(&dispatch_record, "def456", "octocat");
        let decision_for_new_user =
            review_notification_decision(&dispatch_record, "abc123", "hubot");

        assert_eq!(decision_for_new_head, ReviewNotificationDecision::Post);
        assert_eq!(decision_for_new_user, ReviewNotificationDecision::Post);
    }
}
