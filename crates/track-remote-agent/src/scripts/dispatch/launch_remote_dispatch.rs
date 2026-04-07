use serde::Serialize;
use track_types::remote_layout::{DispatchRunDirectory, DispatchWorktreePath};

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const LAUNCH_REMOTE_DISPATCH_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/launch_remote_dispatch.sh.tera");

/// Starts the uploaded launcher in the background for a prepared run
/// directory and worktree.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LaunchRemoteDispatchScript;

impl LaunchRemoteDispatchScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            LAUNCH_REMOTE_DISPATCH_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        remote_run_directory: &DispatchRunDirectory,
        worktree_path: &DispatchWorktreePath,
    ) -> Vec<String> {
        vec![
            remote_run_directory.as_str().to_owned(),
            worktree_path.as_str().to_owned(),
        ]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
