use serde::Serialize;
use track_types::errors::TrackError;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME, REVIEW_RUN_DIRECTORY_NAME,
    REVIEW_WORKTREE_DIRECTORY_NAME,
};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;
use crate::types::RemoteArtifactCleanupCounts;

use super::parse_remote_cleanup_counts;

const CLEANUP_ORPHANED_REMOTE_ARTIFACTS_TEMPLATE: &str =
    include_str!("../../../templates/scripts/cleanup/cleanup_orphaned_remote_artifacts.sh.tera");

/// Removes forgotten dispatch and review artifacts that no longer have local
/// records keeping them alive.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CleanupOrphanedRemoteArtifactsScript;

impl CleanupOrphanedRemoteArtifactsScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CLEANUP_ORPHANED_REMOTE_ARTIFACTS_TEMPLATE,
            &CleanupOrphanedRemoteArtifactsTemplate {
                path_helpers: remote_path_helpers_shell(),
                review_run_directory: REVIEW_RUN_DIRECTORY_NAME,
                review_worktree_directory: REVIEW_WORKTREE_DIRECTORY_NAME,
                launcher_pid_file: REMOTE_LAUNCHER_PID_FILE_NAME,
                codex_pid_file: REMOTE_CODEX_PID_FILE_NAME,
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        workspace_root: &str,
        kept_worktree_paths: &[String],
        kept_run_directories: &[String],
    ) -> Vec<String> {
        let mut arguments = vec![workspace_root.to_owned()];
        arguments.extend(kept_worktree_paths.iter().cloned());
        arguments.push("--".to_owned());
        arguments.extend(kept_run_directories.iter().cloned());
        arguments
    }

    pub(crate) fn parse_report(
        &self,
        report: &str,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        parse_remote_cleanup_counts(report)
    }
}

#[derive(Serialize)]
struct CleanupOrphanedRemoteArtifactsTemplate<'a> {
    path_helpers: &'a str,
    review_run_directory: &'a str,
    review_worktree_directory: &'a str,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
}
