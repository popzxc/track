use serde::Serialize;
use track_types::types::{ReviewRecord, ReviewRunRecord};
use track_types::urls::Url;

use crate::template_renderer::render_template;

const REMOTE_REVIEW_PROMPT_TEMPLATE: &str =
    include_str!("../../templates/prompts/remote_review.md.tera");

/// Renders the review instructions for a prepared remote PR-review run.
///
/// This prompt explains the pull request context, the review constraints, and
/// the follow-up semantics the remote agent should respect while producing the
/// final structured review outcome.
pub(crate) struct RemoteReviewPrompt<'a> {
    review: &'a ReviewRecord,
    dispatch_record: &'a ReviewRunRecord,
    previous_submitted_review: Option<&'a ReviewRunRecord>,
}

impl<'a> RemoteReviewPrompt<'a> {
    pub(crate) fn new(
        review: &'a ReviewRecord,
        dispatch_record: &'a ReviewRunRecord,
        previous_submitted_review: Option<&'a ReviewRunRecord>,
    ) -> Self {
        Self {
            review,
            dispatch_record,
            previous_submitted_review,
        }
    }

    pub(crate) fn render(&self) -> String {
        let branch_name = self
            .dispatch_record
            .branch_name
            .clone()
            .map(|branch_name| branch_name.into_inner())
            .expect("queued review dispatches should always have a branch name");
        let worktree_path = self
            .dispatch_record
            .worktree_path
            .clone()
            .map(|worktree_path| worktree_path.into_inner())
            .expect("queued review dispatches should always have a worktree path");
        let template_context = RemoteReviewPromptTemplate {
            pull_request_url: self.review.pull_request_url.as_str(),
            pull_request_title: &self.review.pull_request_title,
            repository_full_name: &self.review.repository_full_name,
            repo_url: self.review.repo_url.as_str(),
            base_branch: &self.review.base_branch,
            prepared_branch: &branch_name,
            worktree_path: &worktree_path,
            target_head_oid: self.dispatch_record.target_head_oid.as_deref(),
            main_user: &self.review.main_user,
            follow_up_request: self.dispatch_record.follow_up_request.as_deref(),
            show_previous_review_context: self.previous_submitted_review.is_some(),
            previous_github_review_url: self
                .previous_submitted_review
                .and_then(|review| review.github_review_url.as_ref().map(Url::as_str)),
            previous_github_review_id: self
                .previous_submitted_review
                .and_then(|review| review.github_review_id.as_deref()),
            previous_target_head_oid: self
                .previous_submitted_review
                .and_then(|review| review.target_head_oid.as_deref()),
            default_review_prompt: self.review.default_review_prompt.as_deref(),
            extra_instructions: self.review.extra_instructions.as_deref(),
        };

        render_template(REMOTE_REVIEW_PROMPT_TEMPLATE, &template_context)
    }
}

#[derive(Serialize)]
struct RemoteReviewPromptTemplate<'a> {
    pull_request_url: &'a str,
    pull_request_title: &'a str,
    repository_full_name: &'a str,
    repo_url: &'a str,
    base_branch: &'a str,
    prepared_branch: &'a str,
    worktree_path: &'a str,
    target_head_oid: Option<&'a str>,
    main_user: &'a str,
    follow_up_request: Option<&'a str>,
    show_previous_review_context: bool,
    previous_github_review_url: Option<&'a str>,
    previous_github_review_id: Option<&'a str>,
    previous_target_head_oid: Option<&'a str>,
    default_review_prompt: Option<&'a str>,
    extra_instructions: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use track_types::ids::{DispatchId, ProjectId, ReviewId};
    use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, WorkspaceKey};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord,
    };
    use track_types::urls::Url;

    use super::RemoteReviewPrompt;

    fn sample_review_record() -> ReviewRecord {
        let created_at = now_utc();

        ReviewRecord {
            id: ReviewId::new("20260326-120000-review-pr-42").unwrap(),
            pull_request_url: Url::parse("https://github.com/acme/project-x/pull/42").unwrap(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-x".to_owned(),
            repo_url: Url::parse("https://github.com/acme/project-x").unwrap(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            workspace_key: WorkspaceKey::new("project-x").unwrap(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: Some(ProjectId::new("project-x").unwrap()),
            main_user: "octocat".to_owned(),
            default_review_prompt: Some("Focus on regressions and missing tests.".to_owned()),
            extra_instructions: Some("Pay special attention to queue rendering.".to_owned()),
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn builds_remote_review_prompt_with_follow_up_guidance_and_saved_context() {
        let review = sample_review_record();
        let previous_dispatch_id = DispatchId::new("review-dispatch-1").unwrap();
        let previous_review_run = ReviewRunRecord {
            dispatch_id: previous_dispatch_id.clone(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Succeeded,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: Some(now_utc()),
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some(DispatchBranch::for_review(&previous_dispatch_id)),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &review.workspace_key,
                &previous_dispatch_id,
            )),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: Some("Submitted a GitHub review with two inline comments.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(
                Url::parse("https://github.com/acme/project-x/pull/42#pullrequestreview-1001")
                    .unwrap(),
            ),
            notes: None,
            error_message: None,
        };
        let current_dispatch_id = DispatchId::new("review-dispatch-2").unwrap();
        let current_review_run = ReviewRunRecord {
            dispatch_id: current_dispatch_id.clone(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Preparing,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some(DispatchBranch::for_review(&current_dispatch_id)),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &review.workspace_key,
                &current_dispatch_id,
            )),
            follow_up_request: Some(
                "Check whether the main review comments were actually resolved.".to_owned(),
            ),
            target_head_oid: Some("fedcba654321".to_owned()),
            summary: Some(
                "Re-review request: Check whether the main review comments were actually resolved."
                    .to_owned(),
            ),
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };
        let prompt =
            RemoteReviewPrompt::new(&review, &current_review_run, Some(&previous_review_run))
                .render();

        insta::assert_snapshot!(
            "remote_review_prompt_with_follow_up_and_previous_review_context",
            prompt
        );
    }
}
