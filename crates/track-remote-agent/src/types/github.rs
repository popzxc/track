//! Minimal GitHub types used by the remote-agent crate.
//!
//! These structs are based on GitHub API payloads, but they intentionally keep
//! only the fields needed to reason about pull requests, reviews, and remote
//! workspaces. Some DTOs are therefore much smaller than the upstream JSON.

use serde::Deserialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::remote_layout::WorkspaceKey;

/// Identifies a specific pull request in GitHub.
///
/// This is the stable reference the remote-agent logic uses when it needs to
/// query GitHub again, post follow-up feedback, or attach remote work to an
/// existing review conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubPullRequestReference {
    pub(crate) owner: String,
    pub(crate) repository: String,
    pub(crate) number: u64,
}

impl GithubPullRequestReference {
    pub(crate) fn parse(pull_request_url: &str) -> Result<Self, TrackError> {
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

        Ok(Self {
            owner: parts[0].to_owned(),
            repository: parts[1].to_owned(),
            number,
        })
    }

    pub(crate) fn pull_request_endpoint(&self) -> String {
        format!(
            "repos/{}/{}/pulls/{}",
            self.owner, self.repository, self.number
        )
    }

    pub(crate) fn reviews_endpoint(&self) -> String {
        format!("{}/reviews?per_page=100", self.pull_request_endpoint())
    }

    pub(crate) fn issue_comments_endpoint(&self) -> String {
        format!(
            "repos/{}/{}/issues/{}/comments",
            self.owner, self.repository, self.number
        )
    }

    pub(crate) fn repository_full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repository)
    }

    pub(crate) fn repo_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repository)
    }

    pub(crate) fn git_url(&self) -> String {
        format!("git@github.com:{}/{}.git", self.owner, self.repository)
    }
}

/// Captures the pull-request facts needed to create or reuse a remote review
/// workspace.
///
/// The data here is normalized from GitHub responses so the rest of the crate
/// can reason about repository identity, checkout sources, and the head commit
/// without depending on raw API payload shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubPullRequestMetadata {
    pub(crate) pull_request_url: String,
    pub(crate) pull_request_number: u64,
    pub(crate) pull_request_title: String,
    pub(crate) repository_full_name: String,
    pub(crate) repo_url: String,
    pub(crate) git_url: String,
    pub(crate) base_branch: String,
    pub(crate) head_oid: String,
}

impl GithubPullRequestMetadata {
    pub(crate) fn from_api_response(
        reference: &GithubPullRequestReference,
        pull_request_url: &str,
        pull_request: GithubPullRequestApiResponse,
    ) -> Result<Self, TrackError> {
        if pull_request.state != "open" || pull_request.merged_at.is_some() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Pull request {pull_request_url} is not open anymore."),
            ));
        }

        Ok(Self {
            pull_request_url: pull_request_url.trim().to_owned(),
            pull_request_number: reference.number,
            pull_request_title: pull_request.title,
            repository_full_name: reference.repository_full_name(),
            repo_url: reference.repo_url(),
            git_url: reference.git_url(),
            base_branch: pull_request.base.branch_ref,
            head_oid: pull_request.head.sha,
        })
    }

    pub(crate) fn workspace_key(&self) -> WorkspaceKey {
        WorkspaceKey::from_repository_full_name(&self.repository_full_name)
    }
}

/// Describes the review-relevant state of a pull request at one point in time.
///
/// It answers the questions that matter for follow-up automation: whether the
/// pull request can still accept review work, which commit is current, and
/// whether there is already a prior eligible review to build upon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubPullRequestReviewState {
    pub(crate) is_open: bool,
    pub(crate) head_oid: String,
    pub(crate) latest_eligible_review: Option<GithubSubmittedReview>,
}

/// Represents a submitted GitHub review that is relevant to follow-up logic.
///
/// The remote-agent code only needs the review state and submission time in
/// order to decide whether new review work should be requested or existing
/// review context can be reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubSubmittedReview {
    pub(crate) state: String,
    pub(crate) submitted_at: time::OffsetDateTime,
}

impl GithubSubmittedReview {
    pub(crate) fn new(state: String, submitted_at: time::OffsetDateTime) -> Self {
        Self {
            state,
            submitted_at,
        }
    }
}

/// Subset of a GitHub pull-request response used to derive repository and head
/// state.
#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestApiResponse {
    pub(crate) state: String,
    pub(crate) title: String,
    #[serde(rename = "merged_at")]
    pub(crate) merged_at: Option<String>,
    pub(crate) base: GithubPullRequestBaseApiResponse,
    pub(crate) head: GithubPullRequestHeadApiResponse,
}

/// Subset of the base-branch section from a GitHub pull-request response.
#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestBaseApiResponse {
    #[serde(rename = "ref")]
    pub(crate) branch_ref: String,
}

/// Subset of the head-commit section from a GitHub pull-request response.
#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestHeadApiResponse {
    pub(crate) sha: String,
}

/// Subset of the GitHub user payload needed to identify a reviewer.
#[derive(Debug, Deserialize)]
pub(crate) struct GithubUserApiResponse {
    pub(crate) login: String,
}

/// Subset of a GitHub review response used to interpret prior feedback.
///
/// These fields are enough to tell whether a review should count toward
/// follow-up decisions and, if it does, when it was submitted and by whom.
#[derive(Debug, Deserialize)]
pub(crate) struct GithubReviewApiResponse {
    pub(crate) state: String,
    #[serde(rename = "submitted_at")]
    pub(crate) submitted_at: Option<String>,
    pub(crate) user: Option<GithubUserApiResponse>,
}
