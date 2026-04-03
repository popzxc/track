/// Reads the authenticated GitHub login on the remote host.
///
/// The remote-agent crate uses this to discover fork ownership on the remote
/// machine instead of assuming that the configured SSH user matches the GitHub
/// identity used by `gh`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FetchGithubLoginScript;

impl FetchGithubLoginScript {
    pub(crate) fn render(&self) -> String {
        String::from(
            r#"
set -eu
gh api user --jq .login
"#,
        )
    }
}
