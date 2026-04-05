use crate::template_renderer::render_static_template;

const POST_PULL_REQUEST_COMMENT_TEMPLATE: &str =
    include_str!("../../../templates/scripts/dispatch/post_pull_request_comment.sh.tera");

/// Posts a top-level GitHub comment to the pull request's issue thread.
///
/// Review follow-up uses this to notify the configured reviewer about a new PR
/// head without relying on local GitHub state or a separate notification path.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PostPullRequestCommentScript;

impl PostPullRequestCommentScript {
    pub(crate) fn render(&self) -> String {
        render_static_template(POST_PULL_REQUEST_COMMENT_TEMPLATE)
    }

    pub(crate) fn arguments(&self, endpoint: &str, body: &str) -> Vec<String> {
        vec![endpoint.to_owned(), body.to_owned()]
    }
}
