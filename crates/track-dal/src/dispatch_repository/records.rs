use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::parse_iso_8601_millis;
use track_types::types::{DispatchStatus, RemoteAgentPreferredTool, TaskDispatchRecord};

#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskDispatchRow {
    pub(super) dispatch_id: String,
    pub(super) task_id: String,
    pub(super) preferred_tool: String,
    pub(super) project: String,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) remote_host: String,
    pub(super) branch_name: Option<String>,
    pub(super) worktree_path: Option<String>,
    pub(super) pull_request_url: Option<String>,
    pub(super) follow_up_request: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) error_message: Option<String>,
    pub(super) review_request_head_oid: Option<String>,
    pub(super) review_request_user: Option<String>,
}

impl TryFrom<TaskDispatchRow> for TaskDispatchRecord {
    type Error = TrackError;

    fn try_from(record: TaskDispatchRow) -> Result<Self, Self::Error> {
        let dispatch_id = record.dispatch_id;
        let created_at = parse_iso_8601_millis(&record.created_at).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Dispatch {dispatch_id} has an invalid created_at timestamp: {error}"),
            )
        })?;
        let updated_at = parse_iso_8601_millis(&record.updated_at).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Dispatch {dispatch_id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
        let finished_at = record
            .finished_at
            .map(|value| parse_iso_8601_millis(&value))
            .transpose()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!("Dispatch {dispatch_id} has an invalid finished_at timestamp: {error}"),
                )
            })?;

        Ok(TaskDispatchRecord {
            dispatch_id,
            task_id: record.task_id,
            preferred_tool: parse_preferred_tool(record.preferred_tool.as_str())?,
            project: record.project,
            status: parse_dispatch_status(record.status.as_str())?,
            created_at,
            updated_at,
            finished_at,
            remote_host: record.remote_host,
            branch_name: record.branch_name,
            worktree_path: record.worktree_path,
            pull_request_url: record.pull_request_url,
            follow_up_request: record.follow_up_request,
            summary: record.summary,
            notes: record.notes,
            error_message: record.error_message,
            review_request_head_oid: record.review_request_head_oid,
            review_request_user: record.review_request_user,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskIdRow {
    pub(super) task_id: String,
}

fn parse_dispatch_status(value: &str) -> Result<DispatchStatus, TrackError> {
    match value {
        "preparing" => Ok(DispatchStatus::Preparing),
        "running" => Ok(DispatchStatus::Running),
        "succeeded" => Ok(DispatchStatus::Succeeded),
        "canceled" => Ok(DispatchStatus::Canceled),
        "failed" => Ok(DispatchStatus::Failed),
        "blocked" => Ok(DispatchStatus::Blocked),
        _ => Err(TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Dispatch status `{value}` is not valid."),
        )),
    }
}

fn parse_preferred_tool(value: &str) -> Result<RemoteAgentPreferredTool, TrackError> {
    RemoteAgentPreferredTool::from_str(value).ok_or_else(|| {
        TrackError::new(
            ErrorCode::DispatchWriteFailed,
            format!("Remote agent preferred tool `{value}` is not valid."),
        )
    })
}
