//! Types that represent remote dispatch state and remote workspace bookkeeping.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{RemoteAgentPreferredTool, RemoteResetSummary, ReviewRecord};

/// Snapshot of the remote artifacts that describe one dispatch or review run.
///
/// The snapshot is derived from files written on the remote host, so it
/// represents the externally observable state of a run rather than the local
/// database's opinion about that run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RemoteDispatchSnapshot {
    pub(crate) status: Option<String>,
    pub(crate) result: Option<String>,
    pub(crate) stderr: Option<String>,
    pub(crate) finished_at: Option<String>,
}

impl RemoteDispatchSnapshot {
    #[cfg(test)]
    pub(crate) fn completed(result: impl Into<String>, finished_at: impl Into<String>) -> Self {
        Self {
            status: Some("completed\n".to_owned()),
            result: Some(result.into()),
            stderr: None,
            finished_at: Some(finished_at.into()),
        }
    }

    #[cfg(test)]
    pub(crate) fn canceled(finished_at: impl Into<String>) -> Self {
        Self {
            status: Some("canceled\n".to_owned()),
            result: None,
            stderr: None,
            finished_at: Some(finished_at.into()),
        }
    }
}

/// Normalized summary of what a remote cleanup operation actually removed.
///
/// Higher layers use these counts to report cleanup outcomes without leaking the
/// exact JSON shape emitted by the remote helper scripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteArtifactCleanupCounts {
    pub(crate) worktrees_removed: usize,
    pub(crate) run_directories_removed: usize,
}

impl From<RemoteArtifactCleanupReport> for RemoteArtifactCleanupCounts {
    fn from(report: RemoteArtifactCleanupReport) -> Self {
        Self {
            worktrees_removed: report.worktrees_removed,
            run_directories_removed: report.run_directories_removed,
        }
    }
}

/// Raw cleanup report returned by the remote helper script.
///
/// This exists so the script and the Rust code can evolve independently while
/// still sharing a clear, typed contract at the process boundary.
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteArtifactCleanupReport {
    #[serde(rename = "worktreesRemoved")]
    pub(crate) worktrees_removed: usize,
    #[serde(rename = "runDirectoriesRemoved")]
    pub(crate) run_directories_removed: usize,
}

/// Raw report returned by the remote workspace reset helper script.
///
/// The report captures how much persisted remote workspace state was removed so
/// callers can explain the reset outcome without parsing ad hoc shell output.
#[derive(Debug, Deserialize)]
pub(crate) struct RemoteWorkspaceResetReport {
    #[serde(rename = "workspaceEntriesRemoved")]
    pub(crate) workspace_entries_removed: usize,
    #[serde(rename = "registryRemoved")]
    pub(crate) registry_removed: bool,
}

impl RemoteWorkspaceResetReport {
    pub(crate) fn into_summary(self) -> RemoteResetSummary {
        RemoteResetSummary {
            workspace_entries_removed: self.workspace_entries_removed,
            registry_removed: self.registry_removed,
        }
    }
}

/// Persisted registry of repositories that are available on the remote host for
/// dispatch and review workspaces.
///
/// The registry gives the remote-agent logic a durable map from logical project
/// identity to checkout details without rediscovering the remote filesystem on
/// every operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemoteProjectRegistryFile {
    pub(crate) version: u8,
    pub(crate) projects: BTreeMap<String, RemoteProjectRegistryEntry>,
}

/// One project entry inside the remote project registry.
///
/// Each entry describes how the remote host can reach and refresh the checkout
/// that backs future dispatch or review work for that project.
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

impl RemoteProjectRegistryEntry {
    pub(crate) fn from_project_metadata(
        checkout_path: impl Into<String>,
        fork_git_url: impl Into<String>,
        metadata: &ProjectMetadata,
    ) -> Self {
        Self {
            checkout_path: checkout_path.into(),
            fork_git_url: fork_git_url.into(),
            repo_url: metadata.repo_url.clone(),
            git_url: metadata.git_url.clone(),
            base_branch: metadata.base_branch.clone(),
            updated_at: format_iso_8601_millis(now_utc()),
        }
    }

    pub(crate) fn from_review(
        checkout_path: impl Into<String>,
        fork_git_url: impl Into<String>,
        review: &ReviewRecord,
    ) -> Self {
        Self {
            checkout_path: checkout_path.into(),
            fork_git_url: fork_git_url.into(),
            repo_url: review.repo_url.clone(),
            git_url: review.git_url.clone(),
            base_branch: review.base_branch.clone(),
            updated_at: format_iso_8601_millis(now_utc()),
        }
    }
}

impl Default for RemoteProjectRegistryFile {
    fn default() -> Self {
        Self {
            version: 1,
            projects: BTreeMap::new(),
        }
    }
}

/// Describes how aggressively remote artifacts should be removed when a task
/// leaves the active workflow.
///
/// Closing a task and deleting a task are different user intents, so the remote
/// cleanup layer needs an explicit mode instead of inferring semantics from
/// callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteTaskCleanupMode {
    CloseTask,
    DeleteTask,
}

/// Wrapper used when a Claude run returns typed data under a
/// `structured_output` envelope.
///
/// This lets the rest of the crate deserialize the meaningful payload type
/// directly while keeping the provider-specific outer shape at the boundary.
#[derive(Debug, Deserialize)]
pub(crate) struct ClaudeStructuredOutputEnvelope<T> {
    #[serde(rename = "structured_output")]
    pub(crate) structured_output: T,
}

impl<T> ClaudeStructuredOutputEnvelope<T>
where
    T: DeserializeOwned,
{
    pub(crate) fn parse_result(
        raw_result: &str,
        preferred_tool: RemoteAgentPreferredTool,
        result_label: &str,
    ) -> Result<T, TrackError> {
        match serde_json::from_str::<T>(raw_result) {
            Ok(outcome) => Ok(outcome),
            Err(direct_error) if preferred_tool == RemoteAgentPreferredTool::Claude => {
                serde_json::from_str::<ClaudeStructuredOutputEnvelope<T>>(raw_result)
                    .map(|envelope| envelope.structured_output)
                    .map_err(|envelope_error| {
                        TrackError::new(
                            ErrorCode::RemoteDispatchFailed,
                            format!(
                                "{result_label} did not match the expected direct or Claude structured-output format: direct parse failed with {direct_error}; envelope parse failed with {envelope_error}",
                            ),
                        )
                    })
            }
            Err(error) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("{result_label} is not valid JSON: {error}"),
            )),
        }
    }
}
