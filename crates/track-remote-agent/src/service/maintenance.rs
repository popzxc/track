use std::collections::BTreeSet;
use std::sync::Arc;

use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{DispatchStatus, RemoteCleanupSummary, RemoteResetSummary, Status};

use crate::types::RemoteTaskCleanupMode;
use crate::utils::{
    describe_remote_reset_blockers, unique_review_run_directories, unique_review_worktree_paths,
};
use crate::{invalidate_helper_upload, RemoteWorkspace};

use super::dispatch::{unique_run_directories, unique_worktree_paths, RemoteDispatchService};
use super::review::RemoteReviewService;

pub struct RemoteWorkspaceMaintenanceService<'a> {
    database: &'a DatabaseContext,
    workspace: Arc<RemoteWorkspace>,
}

impl<'a> RemoteWorkspaceMaintenanceService<'a> {
    pub(crate) fn new(database: &'a DatabaseContext, workspace: Arc<RemoteWorkspace>) -> Self {
        Self {
            database,
            workspace,
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

    fn dispatch(&self) -> RemoteDispatchService<'a> {
        RemoteDispatchService {
            database: self.database,
            workspace: Arc::clone(&self.workspace),
        }
    }

    fn review(&self) -> RemoteReviewService<'a> {
        RemoteReviewService {
            database: self.database,
            workspace: Arc::clone(&self.workspace),
        }
    }

    // =============================================================================
    // Manual Remote Cleanup
    // =============================================================================
    //
    // Dispatch and review runs are tracked separately, but the user-facing
    // cleanup command has to reconcile both domains against one shared remote
    // workspace. This service owns that cross-domain pass so dispatch/review
    // services can stay focused on their own workflows.
    #[tracing::instrument(skip(self))]
    pub async fn cleanup_unused_remote_artifacts(
        &self,
    ) -> Result<RemoteCleanupSummary, TrackError> {
        let dispatch_service = self.dispatch();
        let remote_agent = self.workspace.remote_agent();
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
                        .extend(unique_run_directories(&dispatch_history, remote_agent));
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
                        .extend(unique_run_directories(&dispatch_history, remote_agent));
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
                            remote_agent,
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

        let orphan_cleanup_counts = self
            .workspace
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
        self.workspace
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
    // Reset is cross-domain by definition: we need both task dispatches and
    // review runs to be idle before we drop the shared remote workspace.
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

        let summary = self.workspace.maintenance().reset_workspace().await?;
        invalidate_helper_upload();
        tracing::warn!(
            workspace_entries_removed = summary.workspace_entries_removed,
            registry_removed = summary.registry_removed,
            "Reset remote workspace"
        );
        Ok(summary)
    }
}
