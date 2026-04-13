//! Typed contracts for the embedded Python remote helper.
//!
//! The helper runs over SSH, but it exposes JSON request and response types so
//! the Rust side can depend on semantic operations instead of shell snippets.

use serde::{Deserialize, Serialize};
use track_types::types::RemoteAgentPreferredTool;

#[derive(Debug, Default, Serialize)]
pub(crate) struct EmptyRequest {}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct EmptyResponse {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnsureCheckoutRequest<'a> {
    pub(crate) repo_url: &'a str,
    pub(crate) repository_name: &'a str,
    pub(crate) git_url: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) checkout_path: &'a str,
    pub(crate) github_login: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnsureCheckoutResponse {
    pub(crate) fork_git_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateWorktreeRequest<'a> {
    pub(crate) checkout_path: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) worktree_path: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateReviewWorktreeRequest<'a> {
    pub(crate) checkout_path: &'a str,
    pub(crate) pull_request_number: u64,
    pub(crate) branch_name: &'a str,
    pub(crate) worktree_path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_head_oid: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnsureFollowUpWorktreeRequest<'a> {
    pub(crate) checkout_path: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) worktree_path: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubApiRequest<'a> {
    pub(crate) endpoint: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubApiResponse {
    pub(crate) output: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubLoginResponse {
    pub(crate) login: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostPullRequestCommentRequest<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) body: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WriteFileRequest<'a> {
    pub(crate) path: &'a str,
    pub(crate) contents: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchRunRequest<'a> {
    pub(crate) run_directory: &'a str,
    pub(crate) worktree_path: &'a str,
    pub(crate) preferred_tool: RemoteAgentPreferredTool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) shell_prelude: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelRunRequest<'a> {
    pub(crate) run_directory: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadRunSnapshotsRequest<'a> {
    pub(crate) run_directories: &'a [String],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReadRunSnapshotsResponse {
    pub(crate) snapshots: Vec<RunSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunSnapshot {
    pub(crate) run_directory: String,
    pub(crate) status: Option<String>,
    pub(crate) result: Option<String>,
    pub(crate) stderr: Option<String>,
    pub(crate) finished_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupTaskArtifactsRequest<'a> {
    pub(crate) checkout_path: &'a str,
    pub(crate) worktree_paths: &'a [String],
    pub(crate) run_directories: &'a [String],
    pub(crate) cleanup_mode: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupReviewArtifactsRequest<'a> {
    pub(crate) checkout_path: &'a str,
    pub(crate) branch_names: &'a [String],
    pub(crate) worktree_paths: &'a [String],
    pub(crate) run_directories: &'a [String],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupOrphanedArtifactsRequest<'a> {
    pub(crate) workspace_root: &'a str,
    pub(crate) keep_worktree_paths: &'a [String],
    pub(crate) keep_run_directories: &'a [String],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupReviewWorkspaceCachesRequest<'a> {
    pub(crate) checkout_paths: &'a [String],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetWorkspaceRequest<'a> {
    pub(crate) workspace_root: &'a str,
    pub(crate) projects_registry_path: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListDirectoriesRequest<'a> {
    pub(crate) path: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListDirectoriesResponse {
    pub(crate) paths: Vec<String>,
}
