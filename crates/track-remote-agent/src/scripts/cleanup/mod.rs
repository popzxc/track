mod cleanup_orphaned_remote_artifacts;
mod cleanup_review_artifacts;
mod cleanup_review_workspace_caches;
mod cleanup_task_artifacts;
mod reset_workspace;

use track_types::errors::{ErrorCode, TrackError};

use crate::types::{RemoteArtifactCleanupCounts, RemoteArtifactCleanupReport};

pub(crate) use cleanup_orphaned_remote_artifacts::CleanupOrphanedRemoteArtifactsScript;
pub(crate) use cleanup_review_artifacts::CleanupReviewArtifactsScript;
pub(crate) use cleanup_review_workspace_caches::CleanupReviewWorkspaceCachesScript;
pub(crate) use cleanup_task_artifacts::CleanupTaskArtifactsScript;
pub(crate) use reset_workspace::ResetWorkspaceScript;

fn parse_remote_cleanup_counts(report: &str) -> Result<RemoteArtifactCleanupCounts, TrackError> {
    let parsed_report = serde_json::from_str::<RemoteArtifactCleanupReport>(report.trim())
        .map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not parse the remote cleanup report: {error}"),
            )
        })?;

    Ok(parsed_report.into())
}
