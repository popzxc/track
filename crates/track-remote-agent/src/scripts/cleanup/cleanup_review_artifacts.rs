use serde::Serialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::remote_layout::{
    DispatchBranch, DispatchRunDirectory, DispatchWorktreePath, RemoteCheckoutPath,
};

use crate::constants::{REMOTE_CODEX_PID_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;
use crate::types::{RemoteArtifactCleanupCounts, RemoteArtifactCleanupReport};

const CLEANUP_REVIEW_ARTIFACTS_TEMPLATE: &str =
    include_str!("../../../templates/scripts/cleanup/cleanup_review_artifacts.sh.tera");

/// Removes review worktrees, review branches, and their run directories.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CleanupReviewArtifactsScript;

impl CleanupReviewArtifactsScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CLEANUP_REVIEW_ARTIFACTS_TEMPLATE,
            &CleanupReviewArtifactsTemplate {
                path_helpers: remote_path_helpers_shell(),
                launcher_pid_file: REMOTE_LAUNCHER_PID_FILE_NAME,
                codex_pid_file: REMOTE_CODEX_PID_FILE_NAME,
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &RemoteCheckoutPath,
        branch_names: &[DispatchBranch],
        worktree_paths: &[DispatchWorktreePath],
        run_directories: &[DispatchRunDirectory],
    ) -> Vec<String> {
        let mut arguments = vec![checkout_path.as_str().to_owned()];
        arguments.extend(
            branch_names
                .iter()
                .map(|branch_name| branch_name.as_str().to_owned()),
        );
        arguments.push("--worktrees".to_owned());
        arguments.extend(
            worktree_paths
                .iter()
                .map(|worktree_path| worktree_path.as_str().to_owned()),
        );
        arguments.push("--runs".to_owned());
        arguments.extend(
            run_directories
                .iter()
                .map(|run_directory| run_directory.as_str().to_owned()),
        );
        arguments
    }

    pub(crate) fn parse_report(
        &self,
        report: &str,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        serde_json::from_str::<RemoteArtifactCleanupReport>(report)
            .map(Into::into)
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote review cleanup report is not valid JSON: {error}"),
                )
            })
    }
}

#[derive(Serialize)]
struct CleanupReviewArtifactsTemplate<'a> {
    path_helpers: &'a str,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
}
