use track_types::errors::TrackError;
use track_types::remote_layout::{DispatchBranch, DispatchRunDirectory, DispatchWorktreePath, RemoteCheckoutPath};
use track_types::types::RemoteResetSummary;

use crate::scripts::{
    CleanupOrphanedRemoteArtifactsScript, CleanupReviewArtifactsScript,
    CleanupReviewWorkspaceCachesScript, CleanupTaskArtifactsScript, ResetWorkspaceScript,
};
use crate::ssh::SshClient;
use crate::types::{RemoteArtifactCleanupCounts, RemoteTaskCleanupMode};

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

    pub(crate) fn execute(&self) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        let script = CleanupTaskArtifactsScript::from_mode(self.cleanup_mode);
        let arguments = script.arguments(
            self.checkout_path,
            self.worktree_paths,
            self.run_directories,
        );
        let report = self.ssh_client.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
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

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = CleanupReviewArtifactsScript;
        let arguments = script.arguments(
            self.checkout_path,
            self.branch_names,
            self.worktree_paths,
            self.run_directories,
        );
        self.ssh_client.run_script(&script.render(), &arguments)?;

        Ok(())
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

    pub(crate) fn execute(&self) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        // The remote workspace layout is currently automation-owned:
        // `<workspace>/<name>/<name>` for the checkout plus sibling
        // task/review worktree and run directories. That lets one broad sweep
        // remove forgotten `dispatch-*` artifacts without needing a second
        // local registry of every worktree ever created.
        // TODO: If the checkout layout ever becomes user-configurable, replace
        // this directory derivation with a registry-backed lookup.
        let script = CleanupOrphanedRemoteArtifactsScript;
        let arguments = script.arguments(
            self.workspace_root,
            self.kept_worktree_paths,
            self.kept_run_directories,
        );
        let report = self.ssh_client.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
    }
}

/// Removes review-only checkout caches that were useful for earlier review
/// runs but no longer have a local reason to stay on the remote machine.
pub(crate) struct CleanupReviewWorkspaceCachesAction<'a> {
    ssh_client: &'a SshClient,
    checkout_paths: &'a [RemoteCheckoutPath],
}

impl<'a> CleanupReviewWorkspaceCachesAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_paths: &'a [RemoteCheckoutPath],
    ) -> Self {
        Self {
            ssh_client,
            checkout_paths,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        if self.checkout_paths.is_empty() {
            return Ok(());
        }

        let script = CleanupReviewWorkspaceCachesScript;
        let arguments = script.arguments(self.checkout_paths);
        self.ssh_client.run_script(&script.render(), &arguments)?;

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

    pub(crate) fn execute(&self) -> Result<RemoteResetSummary, TrackError> {
        let script = ResetWorkspaceScript;
        let arguments = script.arguments(self.workspace_root, self.projects_registry_path);
        let report = self.ssh_client.run_script(&script.render(), &arguments)?;
        script.parse_report(&report)
    }
}
