use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;

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
    pub fn is_active(self) -> bool {
        matches!(self, Self::Preparing | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub project: String,
    pub priority: Priority,
    pub status: Status,
    pub description: String,
    #[serde(rename = "createdAt", with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(rename = "updatedAt", with = "iso_8601_timestamp")]
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
pub struct TaskCreateInput {
    pub project: String,
    pub priority: Priority,
    pub description: String,
    pub source: Option<TaskSource>,
}

impl TaskCreateInput {
    pub fn validate(self) -> Result<Self, TrackError> {
        let validated = Self {
            project: validate_single_normal_path_component(
                &self.project,
                "Task project",
                ErrorCode::InvalidPathComponent,
            )?,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTask {
    pub file_path: PathBuf,
    pub task: Task,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlamaCppModelSource {
    LocalPath(PathBuf),
    HuggingFace { repo: String, file: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaCppRuntimeConfig {
    pub model_source: LlamaCppModelSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRuntimeConfig {
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentReviewFollowUpRuntimeConfig {
    pub main_user: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentDispatchOutcome {
    pub status: DispatchStatus,
    pub summary: String,
    #[serde(rename = "pullRequestUrl", skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<String>,
    #[serde(rename = "branchName", skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDispatchRecord {
    #[serde(rename = "dispatchId")]
    pub dispatch_id: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub project: String,
    pub status: DispatchStatus,
    #[serde(rename = "createdAt", with = "iso_8601_timestamp")]
    pub created_at: OffsetDateTime,
    #[serde(rename = "updatedAt", with = "iso_8601_timestamp")]
    pub updated_at: OffsetDateTime,
    #[serde(
        rename = "finishedAt",
        with = "optional_iso_8601_timestamp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub finished_at: Option<OffsetDateTime>,
    #[serde(rename = "remoteHost")]
    pub remote_host: String,
    #[serde(rename = "branchName", skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    #[serde(rename = "worktreePath", skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(rename = "pullRequestUrl", skip_serializing_if = "Option::is_none")]
    pub pull_request_url: Option<String>,
    #[serde(rename = "followUpRequest", skip_serializing_if = "Option::is_none")]
    pub follow_up_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(rename = "reviewRequestHeadOid", skip_serializing_if = "Option::is_none")]
    pub review_request_head_oid: Option<String>,
    #[serde(rename = "reviewRequestUser", skip_serializing_if = "Option::is_none")]
    pub review_request_user: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCleanupSummary {
    #[serde(rename = "closedTasksCleaned")]
    pub closed_tasks_cleaned: usize,
    #[serde(rename = "missingTasksCleaned")]
    pub missing_tasks_cleaned: usize,
    #[serde(rename = "localDispatchHistoriesRemoved")]
    pub local_dispatch_histories_removed: usize,
    #[serde(rename = "remoteWorktreesRemoved")]
    pub remote_worktrees_removed: usize,
    #[serde(rename = "remoteRunDirectoriesRemoved")]
    pub remote_run_directories_removed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteResetSummary {
    #[serde(rename = "workspaceEntriesRemoved")]
    pub workspace_entries_removed: usize,
    #[serde(rename = "registryRemoved")]
    pub registry_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentRuntimeConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub workspace_root: String,
    pub projects_registry_path: String,
    pub shell_prelude: Option<String>,
    pub review_follow_up: Option<RemoteAgentReviewFollowUpRuntimeConfig>,
    pub managed_key_path: PathBuf,
    pub managed_known_hosts_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRuntimeConfig {
    pub project_roots: Vec<PathBuf>,
    pub project_aliases: BTreeMap<String, String>,
    pub api: ApiRuntimeConfig,
    pub llama_cpp: LlamaCppRuntimeConfig,
    pub remote_agent: Option<RemoteAgentRuntimeConfig>,
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
