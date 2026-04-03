//! Minimal GitHub types used by the remote-agent crate.
//!
//! These structs are based on GitHub API payloads, but they intentionally keep
//! only the fields needed to reason about pull requests, reviews, and remote
//! workspaces. Some DTOs are therefore much smaller than the upstream JSON.

use serde::Deserialize;

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
