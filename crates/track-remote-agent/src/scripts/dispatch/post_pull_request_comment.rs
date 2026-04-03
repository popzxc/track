/// Posts a top-level GitHub comment to the pull request's issue thread.
///
/// Review follow-up uses this to notify the configured reviewer about a new PR
/// head without relying on local GitHub state or a separate notification path.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PostPullRequestCommentScript;

impl PostPullRequestCommentScript {
    pub(crate) fn render(&self) -> String {
        String::from(
            r#"
set -eu
ENDPOINT="$1"
BODY="$2"
gh api --method POST "$ENDPOINT" -f body="$BODY" >/dev/null
"#,
        )
    }

    pub(crate) fn arguments(&self, endpoint: &str, body: &str) -> Vec<String> {
        vec![endpoint.to_owned(), body.to_owned()]
    }
}
