use track_types::errors::TrackError;
use track_types::remote_layout::{
    DispatchBranch, DispatchRunDirectory, DispatchWorktreePath, RemoteCheckoutPath,
};
use track_types::types::RemoteResetSummary;

use crate::helper::{
    CleanupOrphanedArtifactsRequest, CleanupReviewArtifactsRequest,
    CleanupReviewWorkspaceCachesRequest, CleanupTaskArtifactsRequest, EmptyResponse,
    ResetWorkspaceRequest,
};
use crate::ssh::SshClient;
use crate::types::{
    RemoteArtifactCleanupCounts, RemoteArtifactCleanupReport, RemoteTaskCleanupMode,
    RemoteWorkspaceResetReport,
};

/// Removes the remote artifacts owned by one task's dispatch history according
/// to the requested cleanup policy and reports how much state was reclaimed.
pub(crate) struct CleanupTaskArtifactsAction<'a> {
    ssh_client: &'a SshClient,
    checkout_path: &'a RemoteCheckoutPath,
    worktree_paths: &'a [DispatchWorktreePath],
    run_directories: &'a [DispatchRunDirectory],
    cleanup_mode: RemoteTaskCleanupMode,
}

impl<'a> CleanupTaskArtifactsAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_path: &'a RemoteCheckoutPath,
        worktree_paths: &'a [DispatchWorktreePath],
        run_directories: &'a [DispatchRunDirectory],
        cleanup_mode: RemoteTaskCleanupMode,
    ) -> Self {
        Self {
            ssh_client,
            checkout_path,
            worktree_paths,
            run_directories,
            cleanup_mode,
        }
    }

    pub(crate) async fn execute(&self) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let worktree_paths = self
            .worktree_paths
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let run_directories = self
            .run_directories
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let report = self
            .ssh_client
            .run_helper_json::<_, RemoteArtifactCleanupReport>(
                "cleanup-task-artifacts",
                &CleanupTaskArtifactsRequest {
                    checkout_path: self.checkout_path.as_str(),
                    worktree_paths: &worktree_paths,
                    run_directories: &run_directories,
                    cleanup_mode: match self.cleanup_mode {
                        RemoteTaskCleanupMode::CloseTask => "closeTask",
                        RemoteTaskCleanupMode::DeleteTask => "deleteTask",
                    },
                },
            )
            .await?;
        Ok(report.into())
    }
}

/// Removes the remote branches, worktrees, and run directories that belong to
/// saved review runs once that review history no longer needs them.
pub(crate) struct CleanupReviewArtifactsAction<'a> {
    ssh_client: &'a SshClient,
    checkout_path: &'a RemoteCheckoutPath,
    branch_names: &'a [DispatchBranch],
    worktree_paths: &'a [DispatchWorktreePath],
    run_directories: &'a [DispatchRunDirectory],
}

impl<'a> CleanupReviewArtifactsAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_path: &'a RemoteCheckoutPath,
        branch_names: &'a [DispatchBranch],
        worktree_paths: &'a [DispatchWorktreePath],
        run_directories: &'a [DispatchRunDirectory],
    ) -> Self {
        Self {
            ssh_client,
            checkout_path,
            branch_names,
            worktree_paths,
            run_directories,
        }
    }

    pub(crate) async fn execute(&self) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let branch_names = self
            .branch_names
            .iter()
            .map(|branch| branch.as_str().to_owned())
            .collect::<Vec<_>>();
        let worktree_paths = self
            .worktree_paths
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let run_directories = self
            .run_directories
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let report = self
            .ssh_client
            .run_helper_json::<_, RemoteArtifactCleanupReport>(
                "cleanup-review-artifacts",
                &CleanupReviewArtifactsRequest {
                    checkout_path: self.checkout_path.as_str(),
                    branch_names: &branch_names,
                    worktree_paths: &worktree_paths,
                    run_directories: &run_directories,
                },
            )
            .await?;

        Ok(report.into())
    }
}

/// Sweeps the remote workspace for task and review artifacts that are no
/// longer referenced by any local tracker record.
pub(crate) struct CleanupOrphanedRemoteArtifactsAction<'a> {
    ssh_client: &'a SshClient,
    workspace_root: &'a str,
    kept_worktree_paths: &'a [DispatchWorktreePath],
    kept_run_directories: &'a [DispatchRunDirectory],
}

impl<'a> CleanupOrphanedRemoteArtifactsAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        workspace_root: &'a str,
        kept_worktree_paths: &'a [DispatchWorktreePath],
        kept_run_directories: &'a [DispatchRunDirectory],
    ) -> Self {
        Self {
            ssh_client,
            workspace_root,
            kept_worktree_paths,
            kept_run_directories,
        }
    }

    pub(crate) async fn execute(&self) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        // The remote workspace layout is currently automation-owned:
        // `<workspace>/<name>/<name>` for the checkout plus sibling
        // task/review worktree and run directories. That lets one broad sweep
        // remove forgotten `dispatch-*` artifacts without needing a second
        // local registry of every worktree ever created.
        // TODO: If the checkout layout ever becomes user-configurable, replace
        // this directory derivation with a registry-backed lookup.
        let kept_worktree_paths = self
            .kept_worktree_paths
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let kept_run_directories = self
            .kept_run_directories
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        let report = self
            .ssh_client
            .run_helper_json::<_, RemoteArtifactCleanupReport>(
                "cleanup-orphaned-artifacts",
                &CleanupOrphanedArtifactsRequest {
                    workspace_root: self.workspace_root,
                    keep_worktree_paths: &kept_worktree_paths,
                    keep_run_directories: &kept_run_directories,
                },
            )
            .await?;
        Ok(report.into())
    }
}

/// Removes review-only checkout caches that were useful for earlier review
/// runs but no longer have a local reason to stay on the remote machine.
pub(crate) struct CleanupReviewWorkspaceCachesAction<'a> {
    ssh_client: &'a SshClient,
    checkout_paths: &'a [RemoteCheckoutPath],
}

impl<'a> CleanupReviewWorkspaceCachesAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, checkout_paths: &'a [RemoteCheckoutPath]) -> Self {
        Self {
            ssh_client,
            checkout_paths,
        }
    }

    pub(crate) async fn execute(&self) -> Result<(), TrackError> {
        if self.checkout_paths.is_empty() {
            return Ok(());
        }

        let checkout_paths = self
            .checkout_paths
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect::<Vec<_>>();
        self.ssh_client
            .run_helper_json::<_, EmptyResponse>(
                "cleanup-review-workspace-caches",
                &CleanupReviewWorkspaceCachesRequest {
                    checkout_paths: &checkout_paths,
                },
            )
            .await?;

        Ok(())
    }
}

/// Rebuilds the automation-owned remote workspace from scratch while leaving
/// the local tracker state intact.
pub(crate) struct ResetWorkspaceAction<'a> {
    ssh_client: &'a SshClient,
    workspace_root: &'a str,
    projects_registry_path: &'a str,
}

impl<'a> ResetWorkspaceAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        workspace_root: &'a str,
        projects_registry_path: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            workspace_root,
            projects_registry_path,
        }
    }

    pub(crate) async fn execute(&self) -> Result<RemoteResetSummary, TrackError> {
        let report = self
            .ssh_client
            .run_helper_json::<_, RemoteWorkspaceResetReport>(
                "reset-workspace",
                &ResetWorkspaceRequest {
                    workspace_root: self.workspace_root,
                    projects_registry_path: self.projects_registry_path,
                },
            )
            .await?;
        Ok(report.into_summary())
    }
}
