use serde::Serialize;

use crate::constants::{REMOTE_CODEX_PID_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

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
        checkout_path: &str,
        branch_names: &[String],
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Vec<String> {
        let mut arguments = vec![checkout_path.to_owned()];
        arguments.extend(branch_names.iter().cloned());
        arguments.push("--worktrees".to_owned());
        arguments.extend(worktree_paths.iter().cloned());
        arguments.push("--runs".to_owned());
        arguments.extend(run_directories.iter().cloned());
        arguments
    }
}

#[derive(Serialize)]
struct CleanupReviewArtifactsTemplate<'a> {
    path_helpers: &'a str,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
}
