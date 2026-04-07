use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, RemoteCheckoutPath};

use crate::scripts::{
    CreateReviewWorktreeScript, CreateWorktreeScript, EnsureCheckoutScript,
    EnsureFollowUpWorktreeScript,
};
use crate::ssh::SshClient;

/// Ensures the shared remote checkout exists and is ready to serve as the base
/// for future task or review worktrees.
pub(crate) struct EnsureCheckoutAction<'a> {
    ssh_client: &'a SshClient,
    metadata: &'a ProjectMetadata,
    repository_name: &'a str,
    checkout_path: &'a RemoteCheckoutPath,
    github_login: &'a str,
}

impl<'a> EnsureCheckoutAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        metadata: &'a ProjectMetadata,
        repository_name: &'a str,
        checkout_path: &'a RemoteCheckoutPath,
        github_login: &'a str,
    ) -> Self {
        Self {
            ssh_client,
            metadata,
            repository_name,
            checkout_path,
            github_login,
        }
    }

    pub(crate) fn execute(&self) -> Result<GitRemote, TrackError> {
        let script = EnsureCheckoutScript;
        let arguments = script.arguments(
            self.metadata,
            self.repository_name,
            self.checkout_path,
            self.github_login,
        );
        let fork_git_url = self.ssh_client.run_script(&script.render(), &arguments)?;

        let fork_git_url = fork_git_url.trim();
        if fork_git_url.is_empty() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Remote fork setup did not return a fork Git URL.",
            ));
        }

        GitRemote::new(fork_git_url).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Remote fork setup returned an invalid Git remote: {}",
                    error.message()
                ),
            )
        })
    }
}

/// Creates an isolated task worktree so one remote task run can operate on its
/// own branch and filesystem state without mutating the shared checkout.
pub(crate) struct CreateWorktreeAction<'a> {
    ssh_client: &'a SshClient,
    checkout_path: &'a RemoteCheckoutPath,
    base_branch: &'a str,
    branch_name: &'a DispatchBranch,
    worktree_path: &'a DispatchWorktreePath,
}

impl<'a> CreateWorktreeAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_path: &'a RemoteCheckoutPath,
        base_branch: &'a str,
        branch_name: &'a DispatchBranch,
        worktree_path: &'a DispatchWorktreePath,
    ) -> Self {
        Self {
            ssh_client,
            checkout_path,
            base_branch,
            branch_name,
            worktree_path,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = CreateWorktreeScript;
        let arguments = script.arguments(
            self.checkout_path,
            self.base_branch,
            self.branch_name,
            self.worktree_path,
        );
        self.ssh_client.run_script(&script.render(), &arguments)?;

        Ok(())
    }
}

/// Creates a review worktree pinned to a pull request so the remote reviewer
/// inspects the exact code state that the local tracker requested.
pub(crate) struct CreateReviewWorktreeAction<'a> {
    ssh_client: &'a SshClient,
    checkout_path: &'a RemoteCheckoutPath,
    pull_request_number: u64,
    branch_name: &'a DispatchBranch,
    worktree_path: &'a DispatchWorktreePath,
    target_head_oid: Option<&'a str>,
}

impl<'a> CreateReviewWorktreeAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_path: &'a RemoteCheckoutPath,
        pull_request_number: u64,
        branch_name: &'a DispatchBranch,
        worktree_path: &'a DispatchWorktreePath,
        target_head_oid: Option<&'a str>,
    ) -> Self {
        Self {
            ssh_client,
            checkout_path,
            pull_request_number,
            branch_name,
            worktree_path,
            target_head_oid,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = CreateReviewWorktreeScript;
        let arguments = script.arguments(
            self.checkout_path,
            self.pull_request_number,
            self.branch_name,
            self.worktree_path,
            self.target_head_oid,
        );
        self.ssh_client.run_script(&script.render(), &arguments)?;

        Ok(())
    }
}

/// Reuses an existing task worktree for a follow-up run, preserving the prior
/// branch context instead of rebuilding the task environment from scratch.
pub(crate) struct EnsureFollowUpWorktreeAction<'a> {
    ssh_client: &'a SshClient,
    checkout_path: &'a RemoteCheckoutPath,
    branch_name: &'a DispatchBranch,
    worktree_path: &'a DispatchWorktreePath,
}

impl<'a> EnsureFollowUpWorktreeAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        checkout_path: &'a RemoteCheckoutPath,
        branch_name: &'a DispatchBranch,
        worktree_path: &'a DispatchWorktreePath,
    ) -> Self {
        Self {
            ssh_client,
            checkout_path,
            branch_name,
            worktree_path,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = EnsureFollowUpWorktreeScript;
        let arguments = script.arguments(self.checkout_path, self.branch_name, self.worktree_path);
        self.ssh_client.run_script(&script.render(), &arguments)?;

        Ok(())
    }
}
