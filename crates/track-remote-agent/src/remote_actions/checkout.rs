use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, RemoteCheckoutPath};

use crate::helper::{
    CreateReviewWorktreeRequest, CreateWorktreeRequest, EmptyResponse, EnsureCheckoutRequest,
    EnsureCheckoutResponse, EnsureFollowUpWorktreeRequest,
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
        let git_url = self.metadata.git_url.clone().into_remote_string();
        let response = self
            .ssh_client
            .run_helper_json::<_, EnsureCheckoutResponse>(
                "ensure-checkout",
                &EnsureCheckoutRequest {
                    repo_url: self.metadata.repo_url.as_str(),
                    repository_name: self.repository_name,
                    git_url: &git_url,
                    base_branch: &self.metadata.base_branch,
                    checkout_path: self.checkout_path.as_str(),
                    github_login: self.github_login,
                },
            )?;
        let fork_git_url = response.fork_git_url;

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
        self.ssh_client.run_helper_json::<_, EmptyResponse>(
            "create-worktree",
            &CreateWorktreeRequest {
                checkout_path: self.checkout_path.as_str(),
                base_branch: self.base_branch,
                branch_name: self.branch_name.as_str(),
                worktree_path: self.worktree_path.as_str(),
            },
        )?;

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
        self.ssh_client.run_helper_json::<_, EmptyResponse>(
            "create-review-worktree",
            &CreateReviewWorktreeRequest {
                checkout_path: self.checkout_path.as_str(),
                pull_request_number: self.pull_request_number,
                branch_name: self.branch_name.as_str(),
                worktree_path: self.worktree_path.as_str(),
                target_head_oid: self.target_head_oid,
            },
        )?;

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
        self.ssh_client.run_helper_json::<_, EmptyResponse>(
            "ensure-follow-up-worktree",
            &EnsureFollowUpWorktreeRequest {
                checkout_path: self.checkout_path.as_str(),
                branch_name: self.branch_name.as_str(),
                worktree_path: self.worktree_path.as_str(),
            },
        )?;

        Ok(())
    }
}
