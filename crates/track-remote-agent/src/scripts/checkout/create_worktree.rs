use serde::Serialize;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const CREATE_WORKTREE_TEMPLATE: &str =
    include_str!("../../../templates/scripts/checkout/create_worktree.sh.tera");

/// Creates a fresh task worktree from the project's upstream base branch.
///
/// Task dispatches are expected to start from a clean branch rooted at the
/// current upstream base branch, so this script recreates the worktree when
/// necessary instead of trying to repair unknown local state.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CreateWorktreeScript;

impl CreateWorktreeScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CREATE_WORKTREE_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &str,
        base_branch: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Vec<String> {
        vec![
            checkout_path.to_owned(),
            base_branch.to_owned(),
            branch_name.to_owned(),
            worktree_path.to_owned(),
        ]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
