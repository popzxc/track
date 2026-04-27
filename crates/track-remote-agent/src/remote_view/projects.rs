use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::TrackError;
use track_types::ids::ProjectId;
use track_types::remote_layout::{RemoteCheckoutPath, WorkspaceKey};
use track_types::types::ReviewRecord;
use track_types::urls::Url;

use super::types::{
    RemotePullRequestMetadata, RemotePullRequestReviewState, RemoteSubmittedReview,
};
use super::RemoteWorkspace;
use crate::remote_actions::{
    EnsureCheckoutAction, FetchGithubLoginAction, FetchPullRequestMetadataAction,
    FetchPullRequestReviewStateAction, PostPullRequestCommentAction,
};
use crate::types::{GithubPullRequestMetadata, GithubPullRequestReviewState};
use crate::utils::parse_github_repository_name;

pub struct ProjectRemoteRepository<'a> {
    workspace: &'a RemoteWorkspace,
}

impl<'a> ProjectRemoteRepository<'a> {
    pub(super) fn new(workspace: &'a RemoteWorkspace) -> Self {
        Self { workspace }
    }

    pub fn resolve_checkout_path_for_project(&self, project_id: &ProjectId) -> RemoteCheckoutPath {
        self.resolve_checkout_path_for_workspace(&project_id.as_workspace_key())
    }

    pub fn resolve_checkout_path_for_workspace(
        &self,
        workspace_key: &WorkspaceKey,
    ) -> RemoteCheckoutPath {
        RemoteCheckoutPath::for_workspace(
            &self.workspace.remote_agent.workspace_root,
            workspace_key,
        )
    }

    pub async fn ensure_task_checkout(
        &self,
        project_id: &ProjectId,
        metadata: &ProjectMetadata,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let checkout_path = self.resolve_checkout_path_for_project(project_id);
        let github_login = FetchGithubLoginAction::new(&self.workspace.ssh_client)
            .execute()
            .await?;
        let repository_name = parse_github_repository_name(&metadata.repo_url)?;

        EnsureCheckoutAction::new(
            &self.workspace.ssh_client,
            metadata,
            &repository_name,
            &checkout_path,
            &github_login,
        )
        .execute()
        .await?;

        Ok(checkout_path)
    }

    pub async fn ensure_review_checkout(
        &self,
        review: &ReviewRecord,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let review_metadata = ProjectMetadata {
            repo_url: review.repo_url.clone(),
            git_url: review.git_url.clone(),
            base_branch: review.base_branch.clone(),
            description: None,
        };
        let checkout_path = self.resolve_checkout_path_for_workspace(&review.workspace_key);
        let github_login = FetchGithubLoginAction::new(&self.workspace.ssh_client)
            .execute()
            .await?;
        let repository_name = parse_github_repository_name(&review.repo_url)?;

        EnsureCheckoutAction::new(
            &self.workspace.ssh_client,
            &review_metadata,
            &repository_name,
            &checkout_path,
            &github_login,
        )
        .execute()
        .await?;

        Ok(checkout_path)
    }

    pub async fn fetch_pull_request_metadata(
        &self,
        pull_request_url: &Url,
    ) -> Result<RemotePullRequestMetadata, TrackError> {
        let metadata =
            FetchPullRequestMetadataAction::new(&self.workspace.ssh_client, pull_request_url)
                .execute()
                .await?;
        Ok(map_pull_request_metadata(metadata))
    }

    pub async fn fetch_pull_request_review_state(
        &self,
        pull_request_url: &Url,
        main_user: &str,
    ) -> Result<RemotePullRequestReviewState, TrackError> {
        let state = FetchPullRequestReviewStateAction::new(
            &self.workspace.ssh_client,
            pull_request_url,
            main_user,
        )
        .execute()
        .await?;
        Ok(map_pull_request_review_state(state))
    }

    pub async fn post_pull_request_comment(
        &self,
        pull_request_url: &Url,
        comment_body: &str,
    ) -> Result<(), TrackError> {
        PostPullRequestCommentAction::new(
            &self.workspace.ssh_client,
            pull_request_url,
            comment_body,
        )
        .execute()
        .await
    }
}

fn map_pull_request_metadata(metadata: GithubPullRequestMetadata) -> RemotePullRequestMetadata {
    let workspace_key = metadata.workspace_key();

    RemotePullRequestMetadata {
        pull_request_url: metadata.pull_request_url,
        pull_request_number: metadata.pull_request_number,
        pull_request_title: metadata.pull_request_title,
        repository_full_name: metadata.repository_full_name,
        repo_url: metadata.repo_url,
        git_url: metadata.git_url,
        base_branch: metadata.base_branch,
        head_oid: metadata.head_oid,
        workspace_key,
    }
}

fn map_pull_request_review_state(
    state: GithubPullRequestReviewState,
) -> RemotePullRequestReviewState {
    RemotePullRequestReviewState {
        is_open: state.is_open,
        head_oid: state.head_oid,
        latest_eligible_review: state
            .latest_eligible_review
            .map(|review| RemoteSubmittedReview {
                state: review.state,
                submitted_at: review.submitted_at,
            }),
    }
}
