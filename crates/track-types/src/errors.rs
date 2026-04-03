use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    EmptyInput,
    ConfigNotFound,
    InvalidConfig,
    InvalidConfigInput,
    InvalidJson,
    InvalidPathComponent,
    InvalidProjectMetadata,
    InvalidRemoteAgentConfig,
    InvalidTaskUpdate,
    VersionMismatch,
    MigrationFailed,
    MigrationRequired,
    InteractiveRequired,
    NoProjectRoots,
    NoProjectsDiscovered,
    ProjectNotFound,
    InvalidProjectSelection,
    AiParseFailed,
    DispatchNotFound,
    DispatchWriteFailed,
    RemoteAgentNotConfigured,
    RemoteDispatchFailed,
    TaskNotFound,
    ProjectWriteFailed,
    TaskWriteFailed,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct TrackError {
    pub code: ErrorCode,
    message: String,
}

impl TrackError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            ErrorCode::EmptyInput => "EMPTY_INPUT",
            ErrorCode::ConfigNotFound => "CONFIG_NOT_FOUND",
            ErrorCode::InvalidConfig => "INVALID_CONFIG",
            ErrorCode::InvalidConfigInput => "INVALID_CONFIG_INPUT",
            ErrorCode::InvalidJson => "INVALID_JSON",
            ErrorCode::InvalidPathComponent => "INVALID_PATH_COMPONENT",
            ErrorCode::InvalidProjectMetadata => "INVALID_PROJECT_METADATA",
            ErrorCode::InvalidRemoteAgentConfig => "INVALID_REMOTE_AGENT_CONFIG",
            ErrorCode::InvalidTaskUpdate => "INVALID_TASK_UPDATE",
            ErrorCode::VersionMismatch => "VERSION_MISMATCH",
            ErrorCode::MigrationFailed => "MIGRATION_FAILED",
            ErrorCode::MigrationRequired => "MIGRATION_REQUIRED",
            ErrorCode::InteractiveRequired => "INTERACTIVE_REQUIRED",
            ErrorCode::NoProjectRoots => "NO_PROJECT_ROOTS",
            ErrorCode::NoProjectsDiscovered => "NO_PROJECTS_DISCOVERED",
            ErrorCode::ProjectNotFound => "PROJECT_NOT_FOUND",
            ErrorCode::InvalidProjectSelection => "INVALID_PROJECT_SELECTION",
            ErrorCode::AiParseFailed => "AI_PARSE_FAILED",
            ErrorCode::DispatchNotFound => "DISPATCH_NOT_FOUND",
            ErrorCode::DispatchWriteFailed => "DISPATCH_WRITE_FAILED",
            ErrorCode::RemoteAgentNotConfigured => "REMOTE_AGENT_NOT_CONFIGURED",
            ErrorCode::RemoteDispatchFailed => "REMOTE_DISPATCH_FAILED",
            ErrorCode::TaskNotFound => "TASK_NOT_FOUND",
            ErrorCode::ProjectWriteFailed => "PROJECT_WRITE_FAILED",
            ErrorCode::TaskWriteFailed => "TASK_WRITE_FAILED",
        };

        formatter.write_str(code)
    }
}
