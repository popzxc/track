use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::ids::{ProjectId, ReviewId};
use track_types::remote_layout::WorkspaceKey;
use track_types::time_utils::parse_iso_8601_millis;
use track_types::types::{RemoteAgentPreferredTool, ReviewRecord};
use track_types::urls::parse_persisted_url;

#[derive(Debug, sqlx::FromRow)]
pub(super) struct ReviewRow {
    pub(super) id: String,
    pub(super) pull_request_url: String,
    pub(super) pull_request_number: i64,
    pub(super) pull_request_title: String,
    pub(super) repository_full_name: String,
    pub(super) repo_url: String,
    pub(super) git_url: String,
    pub(super) base_branch: String,
    pub(super) workspace_key: String,
    pub(super) preferred_tool: String,
    pub(super) project: Option<String>,
    pub(super) main_user: String,
    pub(super) default_review_prompt: Option<String>,
    pub(super) extra_instructions: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

impl TryFrom<ReviewRow> for ReviewRecord {
    type Error = TrackError;

    fn try_from(record: ReviewRow) -> Result<Self, Self::Error> {
        let id = record.id;
        let created_at = parse_iso_8601_millis(&record.created_at).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Review {id} has an invalid created_at timestamp: {error}"),
            )
        })?;
        let updated_at = parse_iso_8601_millis(&record.updated_at).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Review {id} has an invalid updated_at timestamp: {error}"),
            )
        })?;

        Ok(ReviewRecord {
            id: ReviewId::from_db(id),
            pull_request_url: parse_persisted_url(
                record.pull_request_url,
                "stored review pull request URLs should be valid",
            ),
            pull_request_number: record.pull_request_number as u64,
            pull_request_title: record.pull_request_title,
            repository_full_name: record.repository_full_name,
            repo_url: parse_persisted_url(
                record.repo_url,
                "stored review repo URLs should be valid",
            ),
            git_url: GitRemote::from_db(record.git_url),
            base_branch: record.base_branch,
            workspace_key: WorkspaceKey::from_db_unchecked(record.workspace_key),
            preferred_tool: parse_preferred_tool(record.preferred_tool.as_str())?,
            project: record.project.map(ProjectId::from_db),
            main_user: record.main_user,
            default_review_prompt: record.default_review_prompt,
            extra_instructions: record.extra_instructions,
            created_at,
            updated_at,
        })
    }
}

fn parse_preferred_tool(value: &str) -> Result<RemoteAgentPreferredTool, TrackError> {
    RemoteAgentPreferredTool::from_str(value).ok_or_else(|| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Remote agent preferred tool `{value}` is not valid."),
        )
    })
}
