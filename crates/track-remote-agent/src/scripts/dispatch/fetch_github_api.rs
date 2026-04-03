/// Fetches raw JSON from a GitHub API endpoint via the remote `gh` CLI.
///
/// This keeps GitHub API access inside the same remote execution environment as
/// the rest of the automation, so authentication and network reachability stay
/// consistent across checkout, review, and follow-up workflows.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FetchGithubApiScript;

impl FetchGithubApiScript {
    pub(crate) fn render(&self) -> String {
        String::from(
            r#"
set -eu
ENDPOINT="$1"
gh api "$ENDPOINT"
"#,
        )
    }

    pub(crate) fn arguments(&self, endpoint: &str) -> Vec<String> {
        vec![endpoint.to_owned()]
    }
}
