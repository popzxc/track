use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::errors::{ErrorCode, TrackError};
use crate::git_remote::GitRemote;
use crate::ids::{DispatchId, ProjectId, ReviewId, TaskId};
use crate::remote_layout::{DispatchBranch, DispatchWorktreePath, WorkspaceKey};
use crate::time_utils::now_utc;
use crate::urls::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Closed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskSource {
    Cli,
    Web,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteAgentPreferredTool {
    #[default]
    Codex,
    Claude,
    Opencode,
}

impl RemoteAgentPreferredTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "codex" => Some(Self::Codex),
            "claude" => Some(Self::Claude),
            "opencode" => Some(Self::Opencode),
            _ => None,
        }
    }

    pub fn is_codex(&self) -> bool {
        matches!(self, Self::Codex)
    }

    pub fn is_opencode(&self) -> bool {
        matches!(self, Self::Opencode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DispatchStatus {
    #[serde(alias = "queued")]
    Preparing,
    Running,
    Succeeded,
    Canceled,
    Failed,
    Blocked,
}

impl DispatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Canceled => "canceled",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Preparing | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub project: ProjectId,
    pub priority: Priority,
    pub status: Status,
    pub description: String,
    #[serde(with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "iso_8601_timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<TaskSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTaskCandidate {
    pub project: Option<String>,
    pub priority: Priority,
    pub title: String,
    #[serde(rename = "bodyMarkdown", default)]
    pub body_markdown: Option<String>,
    pub confidence: Confidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreateInput {
    pub project: ProjectId,
    pub priority: Priority,
    pub description: String,
    pub source: Option<TaskSource>,
}

impl TaskCreateInput {
    pub fn validate(self) -> Result<Self, TrackError> {
        let validated = Self {
            project: self.project,
            priority: self.priority,
            description: self.description.trim().to_owned(),
            source: self.source,
        };

        if validated.description.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a task description.",
            ));
        }

        Ok(validated)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdateInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
}

impl TaskUpdateInput {
    pub fn validate(self) -> Result<Self, TrackError> {
        let description = self
            .description
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        let validated = Self {
            description,
            priority: self.priority,
            status: self.status,
        };

        if validated.description.is_none()
            && validated.priority.is_none()
            && validated.status.is_none()
        {
            return Err(TrackError::new(
                ErrorCode::InvalidTaskUpdate,
                "At least one mutable field must be provided.",
            ));
        }

        Ok(validated)
    }
}

// TODO: get rid of this type
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTask {
    pub task: Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentDispatchOutcome {
    pub status: DispatchStatus,
    pub summary: String,
    #[serde(rename = "pullRequestUrl", skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<Url>,
    #[serde(rename = "branchName", skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<DispatchBranch>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: DispatchWorktreePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentReviewOutcome {
    pub status: DispatchStatus,
    pub summary: String,
    #[serde(rename = "reviewSubmitted", default, alias = "reviewPosted")]
    pub review_submitted: bool,
    #[serde(rename = "githubReviewId", default)]
    pub github_review_id: Option<String>,
    #[serde(rename = "githubReviewUrl", default)]
    pub github_review_url: Option<Url>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: DispatchWorktreePath,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDispatchRecord {
    pub dispatch_id: DispatchId,
    pub task_id: TaskId,
    #[serde(default)]
    pub preferred_tool: RemoteAgentPreferredTool,
    pub project: ProjectId,
    pub status: DispatchStatus,
    #[serde(with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "iso_8601_timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(
        with = "optional_iso_8601_timestamp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub finished_at: Option<OffsetDateTime>,
    pub remote_host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<DispatchBranch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<DispatchWorktreePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_request_head_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_request_user: Option<String>,
}

impl TaskDispatchRecord {
    /// Marks a preparing dispatch as actively running once remote reconciliation
    /// observes the launcher making forward progress.
    pub fn mark_running_from_remote(mut self, refreshed_at: OffsetDateTime) -> Self {
        if self.status == DispatchStatus::Preparing {
            self.status = DispatchStatus::Running;
            self.updated_at = refreshed_at;
            self.finished_at = None;
            self.error_message = None;
        }

        self
    }

    /// Marks a preparing dispatch as abandoned when reconciliation can no
    /// longer observe launch progress for longer than the tolerated stale
    /// window.
    pub fn mark_abandoned_if_preparing_stale(
        mut self,
        refreshed_at: OffsetDateTime,
        stale_after: Duration,
    ) -> Option<Self> {
        if self.status != DispatchStatus::Preparing {
            return None;
        }

        if refreshed_at - self.updated_at < stale_after {
            return None;
        }

        self.status = DispatchStatus::Failed;
        self.updated_at = refreshed_at;
        self.finished_at = Some(refreshed_at);
        self.error_message =
            Some("Dispatch preparation stopped before the remote agent launched.".to_owned());
        Some(self)
    }

    /// Records that the remote run was canceled after launch and should now be
    /// treated as terminal locally.
    pub fn mark_canceled_from_remote(
        mut self,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
    ) -> Self {
        self.status = DispatchStatus::Canceled;
        self.updated_at = refreshed_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(
            self.summary
                .unwrap_or_else(|| "Canceled from the web UI.".to_owned()),
        );
        self.error_message = None;
        self
    }

    /// Applies the structured outcome returned by a completed remote task run
    /// to the locally persisted dispatch record.
    pub fn apply_remote_dispatch_outcome(
        mut self,
        outcome: RemoteAgentDispatchOutcome,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
    ) -> Self {
        self.status = outcome.status;
        self.updated_at = refreshed_at;
        self.summary = Some(outcome.summary);
        self.pull_request_url = outcome.pull_request_url;
        let existing_branch_name = self.branch_name.take();
        self.branch_name = outcome.branch_name.or(existing_branch_name);
        self.worktree_path = Some(outcome.worktree_path);
        self.notes = outcome.notes;
        self.error_message = None;
        self.finished_at = Some(finished_at);
        self
    }

    /// Records a terminal refresh failure after the remote run has already
    /// reached a state that should not be retried locally.
    pub fn mark_failed_from_remote_refresh(
        mut self,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
        error_message: impl Into<String>,
    ) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = refreshed_at;
        self.finished_at = Some(finished_at);
        self.error_message = Some(error_message.into());
        self
    }

    pub fn into_preparing(mut self, summary: &str) -> Self {
        self.status = DispatchStatus::Preparing;
        self.summary = Some(summary.to_owned());
        self.updated_at = now_utc();
        self.finished_at = None;
        self.error_message = None;
        self
    }

    pub fn into_running(mut self) -> Self {
        self.status = DispatchStatus::Running;
        self.updated_at = now_utc();
        self.finished_at = None;
        self.summary = Some("The remote agent is working in the prepared environment.".to_owned());
        self.error_message = None;
        self
    }

    pub fn into_failed(mut self, error_message: String) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = now_utc();
        self.finished_at = Some(self.updated_at);
        self.error_message = Some(error_message);
        self
    }

    pub fn into_canceled_from_ui(self) -> Self {
        self.into_locally_finalized(DispatchStatus::Canceled, "Canceled from the web UI.", None)
    }

    pub fn into_locally_finalized(
        mut self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self {
        let finished_at = now_utc();
        self.status = status;
        self.updated_at = finished_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(summary.to_owned());
        self.notes = None;
        self.error_message = error_message.map(ToOwned::to_owned);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRecord {
    pub id: ReviewId,
    pub pull_request_url: Url,
    pub pull_request_number: u64,
    pub pull_request_title: String,
    pub repository_full_name: String,
    pub repo_url: Url,
    pub git_url: GitRemote,
    pub base_branch: String,
    pub workspace_key: WorkspaceKey,
    #[serde(default)]
    pub preferred_tool: RemoteAgentPreferredTool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
    pub main_user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_review_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_instructions: Option<String>,
    #[serde(with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "iso_8601_timestamp")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRunRecord {
    pub dispatch_id: DispatchId,
    pub review_id: ReviewId,
    pub pull_request_url: Url,
    pub repository_full_name: String,
    pub workspace_key: WorkspaceKey,
    #[serde(default)]
    pub preferred_tool: RemoteAgentPreferredTool,
    pub status: DispatchStatus,
    #[serde(with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(with = "iso_8601_timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(
        with = "optional_iso_8601_timestamp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub finished_at: Option<OffsetDateTime>,
    pub remote_host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<DispatchBranch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<DispatchWorktreePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_head_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, alias = "reviewPosted")]
    pub review_submitted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_review_url: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl ReviewRunRecord {
    /// Marks a preparing review run as actively running once the remote review
    /// launcher is confirmed to be making progress.
    pub fn mark_running_from_remote(mut self, refreshed_at: OffsetDateTime) -> Self {
        if self.status == DispatchStatus::Preparing {
            self.status = DispatchStatus::Running;
            self.updated_at = refreshed_at;
            self.finished_at = None;
            self.error_message = None;
        }

        self
    }

    /// Marks a preparing review run as abandoned when reconciliation no longer
    /// sees evidence that the remote review launch is progressing.
    pub fn mark_abandoned_if_preparing_stale(
        mut self,
        refreshed_at: OffsetDateTime,
        stale_after: Duration,
    ) -> Option<Self> {
        if self.status != DispatchStatus::Preparing {
            return None;
        }

        if refreshed_at - self.updated_at < stale_after {
            return None;
        }

        self.status = DispatchStatus::Failed;
        self.updated_at = refreshed_at;
        self.finished_at = Some(refreshed_at);
        self.error_message =
            Some("Review preparation stopped before the remote agent launched.".to_owned());
        Some(self)
    }

    /// Records that the remote review run was canceled after it had already
    /// been launched remotely.
    pub fn mark_canceled_from_remote(
        mut self,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
    ) -> Self {
        self.status = DispatchStatus::Canceled;
        self.updated_at = refreshed_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(
            self.summary
                .unwrap_or_else(|| "Canceled from the web UI.".to_owned()),
        );
        self.error_message = None;
        self
    }

    /// Applies the structured outcome returned by a completed remote review
    /// run to the locally persisted run record.
    pub fn apply_remote_review_outcome(
        mut self,
        outcome: RemoteAgentReviewOutcome,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
    ) -> Self {
        self.status = outcome.status;
        self.updated_at = refreshed_at;
        self.summary = Some(outcome.summary);
        self.review_submitted = outcome.review_submitted;
        self.github_review_id = outcome.github_review_id;
        self.github_review_url = outcome.github_review_url;
        self.worktree_path = Some(outcome.worktree_path);
        self.notes = outcome.notes;
        self.error_message = None;
        self.finished_at = Some(finished_at);
        self
    }

    /// Records a terminal refresh failure after the remote review run already
    /// reached a non-retriable state.
    pub fn mark_failed_from_remote_refresh(
        mut self,
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
        error_message: impl Into<String>,
    ) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = refreshed_at;
        self.finished_at = Some(finished_at);
        self.error_message = Some(error_message.into());
        self
    }

    pub fn into_preparing(mut self, summary: &str) -> Self {
        self.status = DispatchStatus::Preparing;
        self.summary = Some(summary.to_owned());
        self.updated_at = now_utc();
        self.finished_at = None;
        self.error_message = None;
        self
    }

    pub fn into_running(mut self) -> Self {
        self.status = DispatchStatus::Running;
        self.updated_at = now_utc();
        self.finished_at = None;
        self.summary = Some("The remote agent is reviewing the prepared pull request.".to_owned());
        self.error_message = None;
        self
    }

    pub fn into_failed(mut self, error_message: String) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = now_utc();
        self.finished_at = Some(self.updated_at);
        self.error_message = Some(error_message);
        self
    }

    pub fn into_canceled_from_ui(self) -> Self {
        self.into_locally_finalized(DispatchStatus::Canceled, "Canceled from the web UI.", None)
    }

    pub fn into_locally_finalized(
        mut self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self {
        let finished_at = now_utc();
        self.status = status;
        self.updated_at = finished_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(summary.to_owned());
        self.notes = None;
        self.error_message = error_message.map(ToOwned::to_owned);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReviewInput {
    pub pull_request_url: Url,
    #[serde(default)]
    pub preferred_tool: Option<RemoteAgentPreferredTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_instructions: Option<String>,
}

impl CreateReviewInput {
    pub fn validate(self) -> Self {
        let extra_instructions = self
            .extra_instructions
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        Self {
            pull_request_url: self.pull_request_url,
            preferred_tool: self.preferred_tool,
            extra_instructions,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCleanupSummary {
    pub closed_tasks_cleaned: usize,
    pub missing_tasks_cleaned: usize,
    pub local_dispatch_histories_removed: usize,
    pub remote_worktrees_removed: usize,
    pub remote_run_directories_removed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteResetSummary {
    pub workspace_entries_removed: usize,
    pub registry_removed: bool,
}

mod iso_8601_timestamp {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;

    use crate::time_utils::{format_iso_8601_millis, parse_iso_8601_millis};

    pub fn serialize<S>(value: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_iso_8601_millis(*value))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_iso_8601_millis(&value).map_err(serde::de::Error::custom)
    }
}

mod optional_iso_8601_timestamp {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;

    use crate::time_utils::{format_iso_8601_millis, parse_iso_8601_millis};

    pub fn serialize<S>(value: &Option<OffsetDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&format_iso_8601_millis(*value)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|value| parse_iso_8601_millis(&value).map_err(serde::de::Error::custom))
            .transpose()
    }
}
