use crate::template_renderer::render_static_template;

const FETCH_GITHUB_API_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/fetch_github_api.sh.tera");

/// Fetches raw JSON from a GitHub API endpoint via the remote `gh` CLI.
///
/// This keeps GitHub API access inside the same remote execution environment as
/// the rest of the automation, so authentication and network reachability stay
/// consistent across checkout, review, and follow-up workflows.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FetchGithubApiScript;

impl FetchGithubApiScript {
    pub(crate) fn render(&self) -> String {
        render_static_template(FETCH_GITHUB_API_TEMPLATE)
    }

    pub(crate) fn arguments(&self, endpoint: &str) -> Vec<String> {
        vec![endpoint.to_owned()]
    }
}
