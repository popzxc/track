use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use track_types::types::TaskDispatchRecord;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RemoteDispatchSnapshot {
    pub(crate) status: Option<String>,
    pub(crate) result: Option<String>,
    pub(crate) stderr: Option<String>,
    pub(crate) finished_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteArtifactCleanupCounts {
    pub(crate) worktrees_removed: usize,
    pub(crate) run_directories_removed: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteArtifactCleanupReport {
    #[serde(rename = "worktreesRemoved")]
    pub(crate) worktrees_removed: usize,
    #[serde(rename = "runDirectoriesRemoved")]
    pub(crate) run_directories_removed: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteWorkspaceResetReport {
    #[serde(rename = "workspaceEntriesRemoved")]
    pub(crate) workspace_entries_removed: usize,
    #[serde(rename = "registryRemoved")]
    pub(crate) registry_removed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteReviewFollowUpReconciliation {
    pub queued_dispatches: Vec<TaskDispatchRecord>,
    pub review_notifications_updated: usize,
    pub failures: usize,
    pub events: Vec<RemoteReviewFollowUpEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReviewFollowUpEvent {
    pub outcome: String,
    pub detail: String,
    pub task_id: String,
    pub dispatch_id: String,
    pub dispatch_status: String,
    pub remote_host: String,
    pub branch_name: Option<String>,
    pub pull_request_url: Option<String>,
    pub reviewer: String,
    pub pr_is_open: Option<bool>,
    pub pr_head_oid: Option<String>,
    pub latest_review_state: Option<String>,
    pub latest_review_submitted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubPullRequestReference {
    pub(crate) owner: String,
    pub(crate) repository: String,
    pub(crate) number: u64,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubPullRequestReviewState {
    pub(crate) is_open: bool,
    pub(crate) head_oid: String,
    pub(crate) latest_eligible_review: Option<GithubSubmittedReview>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubSubmittedReview {
    pub(crate) state: String,
    pub(crate) submitted_at: time::OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestApiResponse {
    pub(crate) state: String,
    pub(crate) title: String,
    #[serde(rename = "merged_at")]
    pub(crate) merged_at: Option<String>,
    pub(crate) base: GithubPullRequestBaseApiResponse,
    pub(crate) head: GithubPullRequestHeadApiResponse,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestBaseApiResponse {
    #[serde(rename = "ref")]
    pub(crate) branch_ref: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubPullRequestHeadApiResponse {
    pub(crate) sha: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubUserApiResponse {
    pub(crate) login: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GithubReviewApiResponse {
    pub(crate) state: String,
    #[serde(rename = "submitted_at")]
    pub(crate) submitted_at: Option<String>,
    pub(crate) user: Option<GithubUserApiResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteProjectRegistryFile {
    pub(crate) version: u8,
    pub(crate) projects: BTreeMap<String, RemoteProjectRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteProjectRegistryEntry {
    #[serde(rename = "checkoutPath")]
    pub(crate) checkout_path: String,
    #[serde(rename = "forkGitUrl")]
    pub(crate) fork_git_url: String,
    #[serde(rename = "repoUrl")]
    pub(crate) repo_url: String,
    #[serde(rename = "gitUrl")]
    pub(crate) git_url: String,
    #[serde(rename = "baseBranch")]
    pub(crate) base_branch: String,
    #[serde(rename = "updatedAt")]
    pub(crate) updated_at: String,
}

impl Default for RemoteProjectRegistryFile {
    fn default() -> Self {
        Self {
            version: 1,
            projects: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteTaskCleanupMode {
    CloseTask,
    DeleteTask,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeStructuredOutputEnvelope<T> {
    #[serde(rename = "structured_output")]
    pub(crate) structured_output: T,
}
