use serde::Serialize;
use track_types::remote_layout::RemoteCheckoutPath;

use crate::scripts::remote_path_helpers_shell;
use crate::template_renderer::render_template;

const CLEANUP_REVIEW_WORKSPACE_CACHES_TEMPLATE: &str =
    include_str!("../../../templates/scripts/cleanup/cleanup_review_workspace_caches.sh.tera");

/// Removes cached review checkout directories that are no longer needed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CleanupReviewWorkspaceCachesScript;

impl CleanupReviewWorkspaceCachesScript {
    pub(crate) fn render(&self) -> String {
        render_template(
            CLEANUP_REVIEW_WORKSPACE_CACHES_TEMPLATE,
            &PathHelpersTemplate {
                path_helpers: remote_path_helpers_shell(),
            },
        )
    }

    pub(crate) fn arguments(&self, checkout_paths: &[RemoteCheckoutPath]) -> Vec<String> {
        checkout_paths
            .iter()
            .map(|checkout_path| checkout_path.as_str().to_owned())
            .collect()
    }
}

#[derive(Serialize)]
struct PathHelpersTemplate<'a> {
    path_helpers: &'a str,
}
