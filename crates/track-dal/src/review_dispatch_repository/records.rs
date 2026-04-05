use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::{DispatchId, ReviewId};
use track_types::time_utils::parse_iso_8601_millis;
use track_types::types::{DispatchStatus, RemoteAgentPreferredTool, ReviewRunRecord};

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewRunRow {
    pub(super) dispatch_id: String,
    pub(super) review_id: String,
    pub(super) pull_request_url: String,
    pub(super) repository_full_name: String,
    pub(super) workspace_key: String,
    pub(super) preferred_tool: String,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) finished_at: Option<String>,
    pub(super) remote_host: String,
    pub(super) branch_name: Option<String>,
    pub(super) worktree_path: Option<String>,
    pub(super) follow_up_request: Option<String>,
    pub(super) target_head_oid: Option<String>,
    pub(super) summary: Option<String>,
    pub(super) review_submitted: i64,
    pub(super) github_review_id: Option<String>,
    pub(super) github_review_url: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) error_message: Option<String>,
}

impl TryFrom<ReviewRunRow> for ReviewRunRecord {
    type Error = TrackError;

    fn try_from(record: ReviewRunRow) -> Result<Self, Self::Error> {
        let dispatch_id = record.dispatch_id;
        let created_at = parse_iso_8601_millis(&record.created_at).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Review run {dispatch_id} has an invalid created_at timestamp: {error}"),
            )
        })?;
        let updated_at = parse_iso_8601_millis(&record.updated_at).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Review run {dispatch_id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
        let finished_at = record
            .finished_at
            .map(|value| parse_iso_8601_millis(&value))
            .transpose()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Review run {dispatch_id} has an invalid finished_at timestamp: {error}"
                    ),
                )
            })?;

        Ok(ReviewRunRecord {
            dispatch_id: DispatchId::from_db(dispatch_id),
            review_id: ReviewId::from_db(record.review_id),
            pull_request_url: record.pull_request_url,
            repository_full_name: record.repository_full_name,
            workspace_key: record.workspace_key,
            preferred_tool: parse_preferred_tool(record.preferred_tool.as_str())?,
            status: parse_dispatch_status(record.status.as_str())?,
            created_at,
            updated_at,
            finished_at,
            remote_host: record.remote_host,
            branch_name: record.branch_name,
            worktree_path: record.worktree_path,
            follow_up_request: record.follow_up_request,
            target_head_oid: record.target_head_oid,
            summary: record.summary,
            review_submitted: record.review_submitted != 0,
            github_review_id: record.github_review_id,
            github_review_url: record.github_review_url,
            notes: record.notes,
            error_message: record.error_message,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewIdRow {
    pub(super) review_id: String,
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
