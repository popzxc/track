use track_projects::project_metadata::ProjectMetadata;
use track_types::task_description::parse_task_description;
use track_types::time_utils::format_iso_8601_millis;

/// Renders the task-execution instructions for a prepared remote dispatch run.
///
/// This prompt gives the remote coding agent the repository context, the task
/// context, and the behavioral constraints it should follow while producing the
/// final structured dispatch outcome.
pub(crate) struct RemoteDispatchPrompt<'a> {
    project_name: &'a str,
    metadata: &'a ProjectMetadata,
    branch_name: &'a str,
    worktree_path: &'a str,
    task_description: &'a str,
    pull_request_url: Option<&'a str>,
    follow_up_request: Option<&'a str>,
}

impl<'a> RemoteDispatchPrompt<'a> {
    pub(crate) fn new(
        project_name: &'a str,
        metadata: &'a ProjectMetadata,
        branch_name: &'a str,
        worktree_path: &'a str,
        task_description: &'a str,
        pull_request_url: Option<&'a str>,
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
        let mut prompt = String::new();
        prompt.push_str("# Remote task dispatch\n\n");
        prompt.push_str(
            "You are working in a fully autonomous mode on a prepared repository worktree.\n",
        );
        prompt
            .push_str("The repository checkout, fork, and worktree are already set up for you.\n");
        prompt
            .push_str("You have full filesystem access, internet access, and `gh` is available.\n");
        prompt.push_str("Make the decisions needed to complete the task responsibly.\n");
        prompt.push_str(
            "The desired outcome is a GitHub PR unless the task is blocked or cannot be solved.\n\n",
        );
        prompt.push_str("## Repository context\n\n");
        prompt.push_str(&format!("- Project: {}\n", self.project_name));
        prompt.push_str(&format!("- Repo URL: {}\n", self.metadata.repo_url));
        prompt.push_str(&format!("- Git URL: {}\n", self.metadata.git_url));
        prompt.push_str(&format!("- Base branch: {}\n", self.metadata.base_branch));
        prompt.push_str(&format!("- Prepared branch: {}\n", self.branch_name));
        prompt.push_str(&format!("- Working directory: {}\n\n", self.worktree_path));

        if let Some(pull_request_url) = self
            .pull_request_url
            .filter(|value| !value.trim().is_empty())
        {
            prompt.push_str("## Existing PR\n\n");
            prompt.push_str(&format!("- Pull request: {pull_request_url}\n"));
            prompt.push_str(
                "- Continue working on this existing PR with the same prepared branch and worktree.\n",
            );
            prompt.push_str(
                "- Do not open a second PR unless the current PR is unusable and you explain why.\n\n",
            );
        }

        prompt.push_str("## Expectations\n\n");
        prompt.push_str("- Pull the task through to a GitHub PR when possible.\n");
        prompt.push_str("- Use the current worktree as the only place to make changes.\n");
        prompt.push_str("- Use conventional commits for both commit messages and the PR title, for example `feat: Add X`, `fix: Correct Y`, or `chore: Update Z`.\n");
        prompt.push_str("- If the follow-up mentions review comments or reviewer feedback, fetch that context with `gh` instead of guessing.\n");
        prompt.push_str("- If the follow-up names a reviewer, only act on that reviewer's feedback unless the request explicitly says otherwise.\n");
        prompt.push_str(
            "- If the task is blocked, explain the blocker clearly in the final JSON.\n\n",
        );
        prompt.push_str("## Task title\n\n");
        prompt.push_str(&sections.title);
        prompt.push_str("\n\n");

        if let Some(summary_markdown) = sections.summary_markdown.as_deref() {
            prompt.push_str("## Summary\n\n");
            prompt.push_str(summary_markdown);
            prompt.push_str("\n\n");
        }

        if let Some(original_note) = sections.original_note.as_deref() {
            prompt.push_str("## Original note\n\n");
            prompt.push_str(original_note);
            prompt.push_str("\n\n");
        }

        if let Some(follow_up_request) = self
            .follow_up_request
            .filter(|value| !value.trim().is_empty())
        {
            prompt.push_str("## Current follow-up request\n\n");
            prompt.push_str(follow_up_request.trim());
            prompt.push_str("\n\n");
        }

        prompt.push_str("## Final response\n\n");
        prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

        prompt
    }

    /// Renders the follow-up request that asks the remote agent to continue an
    /// existing dispatch from fresh human review feedback.
    ///
    /// This text is not the main dispatch prompt itself, but it belongs to the
    /// same prompt contract because it explains how a later run should scope
    /// its work against the already-open PR.
    pub(crate) fn build_review_follow_up_request(
        pull_request_url: &str,
        main_user: &str,
        dispatch_started_at: time::OffsetDateTime,
    ) -> String {
        format!(
            "Respond to new review feedback from @{main_user} on the existing PR.\n\n\
Use `gh` to fetch submitted PR reviews and inline review comments from @{main_user} only.\n\
Only use reviews with state COMMENTED or CHANGES_REQUESTED that were submitted after {dispatch_started_at}.\n\
Ignore APPROVED reviews and all feedback from other users.\n\
Keep using the existing PR at {pull_request_url} unless you explain why that is impossible.",
            dispatch_started_at = format_iso_8601_millis(dispatch_started_at),
        )
    }
}

#[cfg(test)]
mod tests {
    use track_projects::project_metadata::ProjectMetadata;
    use track_types::task_description::render_task_description;
    use track_types::time_utils::parse_iso_8601_seconds;

    use super::RemoteDispatchPrompt;

    #[test]
    fn builds_remote_prompt_with_both_summary_layers() {
        let prompt = RemoteDispatchPrompt::new(
            "project-x",
            &ProjectMetadata {
                repo_url: "https://github.com/acme/project-x".to_owned(),
                git_url: "git@github.com:acme/project-x.git".to_owned(),
                base_branch: "main".to_owned(),
                description: Some("Main repo".to_owned()),
            },
            "track/dispatch-1",
            "~/workspace/project-x/worktrees/dispatch-1",
            &render_task_description(
                "Fix a bug in module A",
                Some("- Inspect `module_a.rs`"),
                Some("proj-x prio high fix a bug in module A"),
            ),
            Some("https://github.com/acme/project-x/pull/42"),
            Some("Address review comments from the latest PR review."),
        )
        .render();

        assert!(prompt.contains("## Summary"));
        assert!(prompt.contains("## Original note"));
        assert!(prompt.contains("## Existing PR"));
        assert!(prompt.contains("## Current follow-up request"));
        assert!(prompt.contains("fetch that context with `gh`"));
        assert!(prompt.contains("only act on that reviewer's feedback"));
        assert!(prompt.contains("track/dispatch-1"));
        assert!(
            prompt.contains("Use conventional commits for both commit messages and the PR title")
        );
    }

    #[test]
    fn builds_review_follow_up_request_that_scopes_feedback_to_one_user() {
        let request = RemoteDispatchPrompt::build_review_follow_up_request(
            "https://github.com/acme/project-x/pull/42",
            "octocat",
            parse_iso_8601_seconds("2026-03-25T12:00:00Z").expect("timestamp should parse"),
        );

        assert!(request.contains("@octocat"));
        assert!(request.contains("COMMENTED or CHANGES_REQUESTED"));
        assert!(request.contains("Ignore APPROVED reviews"));
        assert!(request.contains("https://github.com/acme/project-x/pull/42"));
    }
}
