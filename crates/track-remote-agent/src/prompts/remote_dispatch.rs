use serde::Serialize;
use track_projects::project_metadata::ProjectMetadata;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath};
use track_types::task_description::parse_task_description;
use track_types::time_utils::format_iso_8601_millis;
use track_types::urls::Url;

use crate::template_renderer::render_template;

const REMOTE_DISPATCH_PROMPT_TEMPLATE: &str =
    include_str!("../../templates/prompts/remote_dispatch.md.tera");
const REVIEW_FOLLOW_UP_REQUEST_TEMPLATE: &str =
    include_str!("../../templates/prompts/review_follow_up_request.txt.tera");

/// Renders the task-execution instructions for a prepared remote dispatch run.
///
/// This prompt gives the remote coding agent the repository context, the task
/// context, and the behavioral constraints it should follow while producing the
/// final structured dispatch outcome.
pub(crate) struct RemoteDispatchPrompt<'a> {
    project_name: &'a str,
    metadata: &'a ProjectMetadata,
    branch_name: &'a DispatchBranch,
    worktree_path: &'a DispatchWorktreePath,
    task_description: &'a str,
    pull_request_url: Option<&'a Url>,
    follow_up_request: Option<&'a str>,
}

impl<'a> RemoteDispatchPrompt<'a> {
    pub(crate) fn new(
        project_name: &'a str,
        metadata: &'a ProjectMetadata,
        branch_name: &'a DispatchBranch,
        worktree_path: &'a DispatchWorktreePath,
        task_description: &'a str,
        pull_request_url: Option<&'a Url>,
        follow_up_request: Option<&'a str>,
    ) -> Self {
        Self {
            project_name,
            metadata,
            branch_name,
            worktree_path,
            task_description,
            pull_request_url,
            follow_up_request,
        }
    }

    pub(crate) fn render(&self) -> String {
        let sections = parse_task_description(self.task_description);
        let git_url = self.metadata.git_url.clone().into_remote_string();
        let template_context = RemoteDispatchPromptTemplate {
            project_name: self.project_name,
            repo_url: self.metadata.repo_url.as_str(),
            git_url: &git_url,
            base_branch: &self.metadata.base_branch,
            branch_name: self.branch_name.as_str(),
            worktree_path: self.worktree_path.as_str(),
            pull_request_url: self
                .pull_request_url
                .map(Url::as_str)
                .and_then(non_empty_trimmed),
            task_title: &sections.title,
            summary_markdown: sections.summary_markdown.as_deref(),
            original_note: sections.original_note.as_deref(),
            follow_up_request: self.follow_up_request.and_then(non_empty_trimmed),
        };

        render_template(REMOTE_DISPATCH_PROMPT_TEMPLATE, &template_context)
    }

    /// Renders the follow-up request that asks the remote agent to continue an
    /// existing dispatch from fresh human review feedback.
    ///
    /// This text is not the main dispatch prompt itself, but it belongs to the
    /// same prompt contract because it explains how a later run should scope
    /// its work against the already-open PR.
    pub(crate) fn build_review_follow_up_request(
        pull_request_url: &Url,
        main_user: &str,
        dispatch_started_at: time::OffsetDateTime,
    ) -> String {
        render_template(
            REVIEW_FOLLOW_UP_REQUEST_TEMPLATE,
            &ReviewFollowUpRequestTemplate {
                pull_request_url: pull_request_url.as_str(),
                main_user,
                dispatch_started_at: format_iso_8601_millis(dispatch_started_at),
            },
        )
    }
}

#[derive(Serialize)]
struct RemoteDispatchPromptTemplate<'a> {
    project_name: &'a str,
    repo_url: &'a str,
    git_url: &'a str,
    base_branch: &'a str,
    branch_name: &'a str,
    worktree_path: &'a str,
    pull_request_url: Option<&'a str>,
    task_title: &'a str,
    summary_markdown: Option<&'a str>,
    original_note: Option<&'a str>,
    follow_up_request: Option<&'a str>,
}

#[derive(Serialize)]
struct ReviewFollowUpRequestTemplate<'a> {
    pull_request_url: &'a str,
    main_user: &'a str,
    dispatch_started_at: String,
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use track_projects::project_metadata::ProjectMetadata;
    use track_types::git_remote::GitRemote;
    use track_types::ids::DispatchId;
    use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath};
    use track_types::task_description::render_task_description;
    use track_types::time_utils::parse_iso_8601_seconds;
    use track_types::urls::Url;

    use super::RemoteDispatchPrompt;

    #[test]
    fn builds_remote_prompt_with_both_summary_layers() {
        let dispatch_id = DispatchId::new("dispatch-1").unwrap();
        let prompt = RemoteDispatchPrompt::new(
            "project-x",
            &ProjectMetadata {
                repo_url: Url::parse("https://github.com/acme/project-x").unwrap(),
                git_url: GitRemote::new("git@github.com:acme/project-x.git").unwrap(),
                base_branch: "main".to_owned(),
                description: Some("Main repo".to_owned()),
            },
            &DispatchBranch::for_task(&dispatch_id),
            &DispatchWorktreePath::for_task(
                "~/workspace",
                &track_types::ids::ProjectId::new("project-x").unwrap(),
                &dispatch_id,
            ),
            &render_task_description(
                "Fix a bug in module A",
                Some("- Inspect `module_a.rs`"),
                Some("proj-x prio high fix a bug in module A"),
            ),
            Some(&Url::parse("https://github.com/acme/project-x/pull/42").unwrap()),
            Some("Address review comments from the latest PR review."),
        )
        .render();

        insta::assert_snapshot!(
            "remote_dispatch_prompt_with_existing_pr_and_follow_up",
            prompt
        );
    }

    #[test]
    fn builds_review_follow_up_request_that_scopes_feedback_to_one_user() {
        let request = RemoteDispatchPrompt::build_review_follow_up_request(
            &Url::parse("https://github.com/acme/project-x/pull/42").unwrap(),
            "octocat",
            parse_iso_8601_seconds("2026-03-25T12:00:00Z").expect("timestamp should parse"),
        );

        insta::assert_snapshot!("review_follow_up_request", request);
    }
}
