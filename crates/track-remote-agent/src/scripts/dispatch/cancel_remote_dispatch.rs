use serde::Serialize;
use track_types::remote_layout::DispatchRunDirectory;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_STATUS_FILE_NAME,
};
use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const CANCEL_REMOTE_DISPATCH_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/cancel_remote_dispatch.sh.tera");

/// Cancels an active remote run by killing the launcher and model processes and
/// marking the run as canceled.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CancelRemoteDispatchScript;

impl CancelRemoteDispatchScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CANCEL_REMOTE_DISPATCH_TEMPLATE,
            &CancelRemoteDispatchTemplate {
                path_helpers: remote_path_helpers_shell(),
                launcher_pid_file: REMOTE_LAUNCHER_PID_FILE_NAME,
                codex_pid_file: REMOTE_CODEX_PID_FILE_NAME,
                status_file: REMOTE_STATUS_FILE_NAME,
                finished_at_file: REMOTE_FINISHED_AT_FILE_NAME,
            },
        )
    }

    pub(crate) fn arguments(&self, remote_run_directory: &DispatchRunDirectory) -> Vec<String> {
        vec![remote_run_directory.as_str().to_owned()]
    }
}

#[derive(Serialize)]
struct CancelRemoteDispatchTemplate<'a> {
    path_helpers: &'a str,
    launcher_pid_file: &'a str,
    codex_pid_file: &'a str,
    status_file: &'a str,
    finished_at_file: &'a str,
}
