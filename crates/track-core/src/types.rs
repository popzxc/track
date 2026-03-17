use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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
    pub description: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCreateInput {
    pub project: String,
    pub priority: Priority,
    pub description: String,
    pub source: Option<TaskSource>,
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
    pub fn validate(self) -> Result<Self, crate::errors::TrackError> {
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
            return Err(crate::errors::TrackError::new(
                crate::errors::ErrorCode::InvalidTaskUpdate,
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
pub struct LlamaCppRuntimeConfig {
    pub model_path: PathBuf,
    pub llama_completion_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRuntimeConfig {
    pub project_roots: Vec<PathBuf>,
    pub project_aliases: BTreeMap<String, String>,
    pub llama_cpp: LlamaCppRuntimeConfig,
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
