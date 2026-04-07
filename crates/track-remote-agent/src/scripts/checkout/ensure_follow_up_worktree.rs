use serde::Serialize;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, RemoteCheckoutPath};

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const ENSURE_FOLLOW_UP_WORKTREE_TEMPLATE: &str =
    include_str!("../../../templates/scripts/checkout/ensure_follow_up_worktree.sh.tera");

/// Reuses the existing branch worktree for a follow-up dispatch when possible.
///
/// Follow-up runs should keep working in the same branch context as the
/// original dispatch, so this script restores that worktree instead of creating
/// a brand-new branch from upstream.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EnsureFollowUpWorktreeScript;

impl EnsureFollowUpWorktreeScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            ENSURE_FOLLOW_UP_WORKTREE_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &RemoteCheckoutPath,
        branch_name: &DispatchBranch,
        worktree_path: &DispatchWorktreePath,
    ) -> Vec<String> {
        vec![
            checkout_path.as_str().to_owned(),
            branch_name.as_str().to_owned(),
            worktree_path.as_str().to_owned(),
        ]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
