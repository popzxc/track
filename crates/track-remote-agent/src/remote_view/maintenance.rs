use track_types::errors::TrackError;
use track_types::remote_layout::{DispatchRunDirectory, DispatchWorktreePath, WorkspaceKey};
use track_types::types::RemoteResetSummary;

use crate::remote_actions::{
    CleanupOrphanedRemoteArtifactsAction, CleanupReviewWorkspaceCachesAction, ResetWorkspaceAction,
};

use super::types::RemoteArtifactCleanupSummary;
use super::RemoteWorkspace;

pub struct RemoteMaintenanceRepository<'a> {
    workspace: &'a RemoteWorkspace,
}

impl<'a> RemoteMaintenanceRepository<'a> {
    pub(super) fn new(workspace: &'a RemoteWorkspace) -> Self {
        Self { workspace }
    }

    pub fn cleanup_orphaned_artifacts(
        &self,
        kept_worktree_paths: &[DispatchWorktreePath],
        kept_run_directories: &[DispatchRunDirectory],
    ) -> Result<RemoteArtifactCleanupSummary, TrackError> {
        let counts = CleanupOrphanedRemoteArtifactsAction::new(
            &self.workspace.ssh_client,
            &self.workspace.remote_agent.workspace_root,
            kept_worktree_paths,
            kept_run_directories,
        )
        .execute()?;

        Ok(RemoteArtifactCleanupSummary {
            worktrees_removed: counts.worktrees_removed,
            run_directories_removed: counts.run_directories_removed,
        })
    }

    pub fn cleanup_reclaimable_review_workspaces(
        &self,
        workspace_keys: &[WorkspaceKey],
    ) -> Result<(), TrackError> {
        if workspace_keys.is_empty() {
            return Ok(());
        }

        let checkout_paths = workspace_keys
            .iter()
            .map(|workspace_key| {
                self.workspace
                    .projects()
                    .resolve_checkout_path_for_workspace(workspace_key)
            })
            .collect::<Vec<_>>();

        CleanupReviewWorkspaceCachesAction::new(&self.workspace.ssh_client, &checkout_paths)
            .execute()
    }

    pub fn reset_workspace(&self) -> Result<RemoteResetSummary, TrackError> {
        ResetWorkspaceAction::new(
            &self.workspace.ssh_client,
            &self.workspace.remote_agent.workspace_root,
            &self.workspace.remote_agent.projects_registry_path,
        )
        .execute()
    }
}
