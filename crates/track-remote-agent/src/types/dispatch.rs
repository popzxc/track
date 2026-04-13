//! Types that represent remote dispatch state and remote workspace bookkeeping.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{RemoteAgentPreferredTool, RemoteResetSummary};

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
