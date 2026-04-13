#[cfg(test)]
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use track_projects::project_metadata::ProjectRecord;
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::remote_layout::{DispatchRunDirectory, DispatchWorktreePath, WorkspaceKey};
use track_types::time_utils::parse_iso_8601_seconds;
use track_types::types::{ReviewRecord, ReviewRunRecord, TaskDispatchRecord};
use track_types::urls::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePullRequestMetadata {
    pub pull_request_url: Url,
    pub pull_request_number: u64,
    pub pull_request_title: String,
    pub repository_full_name: String,
    pub repo_url: Url,
    pub git_url: GitRemote,
    pub base_branch: String,
    pub head_oid: String,
    pub workspace_key: WorkspaceKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSubmittedReview {
    pub state: String,
    pub submitted_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePullRequestReviewState {
    pub is_open: bool,
    pub head_oid: String,
    pub latest_eligible_review: Option<RemoteSubmittedReview>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteWorktreeKind {
    Task,
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteWorktreeEntry {
    pub kind: RemoteWorktreeKind,
    pub path: DispatchWorktreePath,
    pub run_directory: DispatchRunDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTaskArtifactCleanupMode {
    CloseTask,
    DeleteTask,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteArtifactCleanupSummary {
    pub worktrees_removed: usize,
    pub run_directories_removed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRunObservedStatus {
    Missing,
    Preparing,
    Running,
    Completed,
    Canceled,
    Failed,
    Unexpected(String),
}

impl RemoteRunObservedStatus {
    pub(crate) fn from_status_file_contents(contents: Option<String>) -> Self {
        let Some(contents) = contents else {
            return Self::Missing;
        };
        let normalized = contents.trim();
        if normalized.is_empty() {
            return Self::Missing;
        }

        match normalized {
            "preparing" => Self::Preparing,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "canceled" => Self::Canceled,
            "launcher_failed" => Self::Failed,
            other => Self::Unexpected(other.to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRunSnapshotView {
    pub run_directory: DispatchRunDirectory,
    pub status: RemoteRunObservedStatus,
    pub result: Option<String>,
    pub stderr: Option<String>,
    pub finished_at: Option<String>,
}

impl RemoteRunSnapshotView {
    pub(crate) fn missing(run_directory: DispatchRunDirectory) -> Self {
        Self {
            run_directory,
            status: RemoteRunObservedStatus::Missing,
            result: None,
            stderr: None,
            finished_at: None,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self.status, RemoteRunObservedStatus::Missing)
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, RemoteRunObservedStatus::Running)
    }

    pub fn is_canceled(&self) -> bool {
        matches!(self.status, RemoteRunObservedStatus::Canceled)
    }

    pub fn is_completed(&self) -> bool {
        matches!(self.status, RemoteRunObservedStatus::Completed)
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            RemoteRunObservedStatus::Completed | RemoteRunObservedStatus::Failed
        )
    }

    pub fn finished_at_or(&self, fallback: OffsetDateTime) -> OffsetDateTime {
        self.finished_at
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| parse_iso_8601_seconds(value).ok())
            .unwrap_or(fallback)
    }

    pub fn required_result(&self, missing_message: &'static str) -> Result<&str, TrackError> {
        self.result
            .as_deref()
            .ok_or_else(|| TrackError::new(ErrorCode::RemoteDispatchFailed, missing_message))
    }

    pub fn failure_message(&self, fallback_message: &'static str) -> String {
        self.stderr
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| fallback_message.to_owned())
    }

    #[cfg(test)]
    pub fn completed(result: impl Into<String>, finished_at: OffsetDateTime) -> Self {
        Self {
            run_directory: DispatchRunDirectory::from_db_unchecked("/test/dispatches/run"),
            status: RemoteRunObservedStatus::Completed,
            result: Some(result.into()),
            stderr: None,
            finished_at: Some(
                finished_at
                    .format(&Rfc3339)
                    .expect("test timestamp should format as RFC 3339"),
            ),
        }
    }

    #[cfg(test)]
    pub fn canceled(finished_at: OffsetDateTime) -> Self {
        Self {
            run_directory: DispatchRunDirectory::from_db_unchecked("/test/dispatches/run"),
            status: RemoteRunObservedStatus::Canceled,
            result: None,
            stderr: None,
            finished_at: Some(
                finished_at
                    .format(&Rfc3339)
                    .expect("test timestamp should format as RFC 3339"),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDispatchView {
    pub record: TaskDispatchRecord,
    pub remote: RemoteRunSnapshotView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRunView {
    pub record: ReviewRunRecord,
    pub remote: RemoteRunSnapshotView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteProjectSnapshot {
    pub project: ProjectRecord,
    pub task_dispatches: Vec<TaskDispatchView>,
    pub reviews: Vec<ReviewRecord>,
    pub review_runs: Vec<ReviewRunView>,
    pub task_worktrees: Vec<RemoteWorktreeEntry>,
    pub review_worktrees: Vec<RemoteWorktreeEntry>,
}
