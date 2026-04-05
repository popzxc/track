use serde::Serialize;
use track_types::errors::TrackError;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_STATUS_FILE_NAME,
};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;
use crate::types::{RemoteArtifactCleanupCounts, RemoteTaskCleanupMode};

use super::parse_remote_cleanup_counts;

const CLEANUP_TASK_ARTIFACTS_TEMPLATE: &str =
    include_str!("../../../templates/scripts/cleanup/cleanup_task_artifacts.sh.tera");

/// Removes task-owned remote worktrees and, when appropriate, their run
/// directories.
///
/// Closing and deleting a task have different retention semantics, so this
/// script accepts an explicit cleanup mode rather than inferring the desired
/// behavior from the caller.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CleanupTaskArtifactsScript {
    cleanup_remote_dispatch_directories: bool,
}

impl CleanupTaskArtifactsScript {
    pub(crate) fn from_mode(cleanup_mode: RemoteTaskCleanupMode) -> Self {
        Self {
            cleanup_remote_dispatch_directories: cleanup_mode == RemoteTaskCleanupMode::DeleteTask,
        }
    }

    pub(crate) fn render(&self) -> String {
        render_template(
            CLEANUP_TASK_ARTIFACTS_TEMPLATE,
            &CleanupTaskArtifactsTemplate {
                path_helpers: remote_path_helpers_shell(),
                cleanup_remote_dispatch_directories: self.cleanup_remote_dispatch_directories,
                launcher_pid_file: REMOTE_LAUNCHER_PID_FILE_NAME,
                codex_pid_file: REMOTE_CODEX_PID_FILE_NAME,
                status_file: REMOTE_STATUS_FILE_NAME,
                finished_at_file: REMOTE_FINISHED_AT_FILE_NAME,
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &str,
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Vec<String> {
        let mut arguments = vec![checkout_path.to_owned()];
        arguments.extend(worktree_paths.iter().cloned());
        arguments.push("--".to_owned());
        arguments.extend(run_directories.iter().cloned());
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
struct CleanupTaskArtifactsTemplate<'a> {
    path_helpers: &'a str,
    cleanup_remote_dispatch_directories: bool,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
    status_file: &'a str,
    finished_at_file: &'a str,
}
