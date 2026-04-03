//! Types that represent remote dispatch state and remote workspace bookkeeping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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

/// Normalized summary of what a remote cleanup operation actually removed.
///
/// Higher layers use these counts to report cleanup outcomes without leaking the
/// exact JSON shape emitted by the remote helper scripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RemoteArtifactCleanupCounts {
    pub(crate) worktrees_removed: usize,
    pub(crate) run_directories_removed: usize,
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
