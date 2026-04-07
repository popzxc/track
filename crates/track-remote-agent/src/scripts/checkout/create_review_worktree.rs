use serde::Serialize;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, RemoteCheckoutPath};

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const CREATE_REVIEW_WORKTREE_TEMPLATE: &str =
    include_str!("../../../templates/scripts/checkout/create_review_worktree.sh.tera");

/// Creates a review worktree pinned to the pull request head the review was
/// queued against.
///
/// Review work needs a reproducible snapshot of the pull request, so this
/// script refreshes the GitHub PR ref but fails explicitly if the requested
/// commit can no longer be materialized.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CreateReviewWorktreeScript;

impl CreateReviewWorktreeScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CREATE_REVIEW_WORKTREE_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &RemoteCheckoutPath,
        pull_request_number: u64,
        branch_name: &DispatchBranch,
        worktree_path: &DispatchWorktreePath,
        target_head_oid: Option<&str>,
    ) -> Vec<String> {
        vec![
            checkout_path.as_str().to_owned(),
            pull_request_number.to_string(),
            branch_name.as_str().to_owned(),
            worktree_path.as_str().to_owned(),
            target_head_oid.unwrap_or_default().to_owned(),
        ]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}

#[cfg(test)]
mod tests {
    use super::CreateReviewWorktreeScript;

    #[test]
    fn pins_the_requested_commit_or_fails_explicitly() {
        let script = CreateReviewWorktreeScript.render();

        insta::assert_snapshot!("create_review_worktree_script", script);
    }
}
