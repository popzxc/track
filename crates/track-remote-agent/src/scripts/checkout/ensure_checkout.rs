use serde::Serialize;
use track_projects::project_metadata::ProjectMetadata;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const ENSURE_CHECKOUT_TEMPLATE: &str =
    include_str!("../../../templates/scripts/checkout/ensure_checkout.sh.tera");

/// Prepares the canonical remote checkout for a project.
///
/// This script makes sure the remote host has a usable clone, that `origin`
/// points at the user's fork, and that `upstream` points at the source
/// repository. Later task and review worktrees assume this checkout is the
/// stable base they can branch from.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EnsureCheckoutScript;

impl EnsureCheckoutScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            ENSURE_CHECKOUT_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(
        &self,
        metadata: &ProjectMetadata,
        repository_name: &str,
        checkout_path: &str,
        github_login: &str,
    ) -> Vec<String> {
        vec![
            metadata.repo_url.to_string(),
            repository_name.to_owned(),
            metadata.git_url.clone().into_remote_string(),
            metadata.base_branch.clone(),
            checkout_path.to_owned(),
            github_login.to_owned(),
        ]
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
