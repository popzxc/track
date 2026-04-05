use crate::template_renderer::render_static_template;

const FETCH_GITHUB_LOGIN_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/fetch_github_login.sh.tera");

/// Reads the authenticated GitHub login on the remote host.
///
/// The remote-agent crate uses this to discover fork ownership on the remote
/// machine instead of assuming that the configured SSH user matches the GitHub
/// identity used by `gh`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FetchGithubLoginScript;

impl FetchGithubLoginScript {
    pub(crate) fn render(&self) -> String {
        render_static_template(FETCH_GITHUB_LOGIN_TEMPLATE)
    }
}
