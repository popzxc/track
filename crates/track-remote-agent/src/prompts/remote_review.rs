use track_types::types::{ReviewRecord, ReviewRunRecord};

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
            .as_deref()
            .expect("queued review dispatches should always have a branch name");
        let worktree_path = self
            .dispatch_record
            .worktree_path
            .as_deref()
            .expect("queued review dispatches should always have a worktree path");

        let mut prompt = String::new();
        prompt.push_str("# Remote PR review\n\n");
        prompt.push_str(
            "You are reviewing an existing GitHub pull request from a prepared repository worktree.\n",
        );
        prompt.push_str(
            "The repository checkout and review worktree are already prepared for you.\n",
        );
        prompt
            .push_str("You have full filesystem access, internet access, and `gh` is available.\n");
        prompt.push_str("This run is for review only: do not push commits, open PRs, or request reviewers yourself.\n");
        prompt.push_str("You are responsible for submitting the GitHub review yourself before you return the final JSON.\n\n");
        prompt.push_str("## Pull request context\n\n");
        prompt.push_str(&format!(
            "- Pull request: {}\n",
            self.review.pull_request_url
        ));
        prompt.push_str(&format!("- Title: {}\n", self.review.pull_request_title));
        prompt.push_str(&format!(
            "- Repository: {}\n",
            self.review.repository_full_name
        ));
        prompt.push_str(&format!("- Repo URL: {}\n", self.review.repo_url));
        prompt.push_str(&format!("- Base branch: {}\n", self.review.base_branch));
        prompt.push_str(&format!("- Prepared branch: {branch_name}\n"));
        prompt.push_str(&format!("- Working directory: {worktree_path}\n"));
        if let Some(target_head_oid) = self.dispatch_record.target_head_oid.as_deref() {
            prompt.push_str(&format!("- Pinned review commit: {target_head_oid}\n"));
        }
        prompt.push('\n');
        prompt.push_str("## Review instructions\n\n");
        prompt.push_str("- Submit one GitHub review in COMMENT mode.\n");
        prompt.push_str(&format!(
            "- The first line of the top-level review body must be `@{} requested me to review this PR.`\n",
            self.review.main_user
        ));
        prompt.push_str("- Prefer inline review comments for concrete file/line findings so people can reply in GitHub threads.\n");
        prompt.push_str("- Use the top-level review body for the overall summary, major risks, and any no-findings conclusion.\n");
        prompt.push_str(
            "- Focus on bugs, regressions, risky behavior changes, missing tests, and edge cases.\n",
        );
        prompt.push_str("- Use the checked-out code and `gh` to inspect the PR diff and context instead of guessing.\n");
        prompt.push_str("- If a pinned review commit is listed above, the prepared worktree is intended to match that exact commit. If it does not, stop and explain the mismatch instead of reviewing a newer head silently.\n");
        prompt.push_str("- Keep the review concise but concrete.\n");
        prompt.push_str(
            "- If you do not find problems, say so explicitly in the top-level review body.\n",
        );
        prompt.push_str("- If you cannot complete the review responsibly, explain the blocker in the summary and do not claim the review was submitted.\n");
        prompt.push_str("- Capture the submitted GitHub review's durable handle from the `gh` response and return it as `githubReviewId` and `githubReviewUrl` when submission succeeds.\n");
        prompt.push_str("- Return `reviewSubmitted` as `true` only after GitHub confirms that the review submission succeeded.\n\n");

        if let Some(follow_up_request) = self.dispatch_record.follow_up_request.as_deref() {
            prompt.push_str("## Current re-review request\n\n");
            prompt.push_str(follow_up_request.trim());
            prompt.push_str("\n\n");
        }

        if let Some(previous_submitted_review) = self.previous_submitted_review {
            prompt.push_str("## Previous bot review context\n\n");
            if let Some(github_review_url) = previous_submitted_review.github_review_url.as_deref()
            {
                prompt.push_str(&format!(
                    "- Previous submitted review: {github_review_url}\n"
                ));
            }
            if let Some(github_review_id) = previous_submitted_review.github_review_id.as_deref() {
                prompt.push_str(&format!(
                    "- Previous submitted review id: {github_review_id}\n"
                ));
            }
            if let Some(target_head_oid) = previous_submitted_review.target_head_oid.as_deref() {
                prompt.push_str(&format!(
                    "- Previous review pinned commit: {target_head_oid}\n"
                ));
            }
            prompt.push('\n');
            prompt.push_str("## Re-review guidance\n\n");
            prompt.push_str("- Inspect the current PR conversation on GitHub before deciding whether an older bot finding still matters.\n");
            prompt.push_str(&format!(
                "- For context: your previous comments are always non-blocking input at the discretion of the reviewee unless @{} explicitly commented that a finding is valid and should be fixed.\n",
                self.review.main_user
            ));
            prompt.push_str(&format!(
                "- Only treat an older bot finding as something you must actively verify and potentially elevate into a primary finding if @{} explicitly said it is valid and should be fixed.\n",
                self.review.main_user
            ));
            prompt.push_str(&format!(
                "- If @{} or the reviewee explicitly said an older bot finding is not important, disputed it, or chose not to address it, do not repeat it as a primary finding just because it appeared in a previous bot review.\n",
                self.review.main_user
            ));
            prompt.push_str("- You may mention unresolved prior bot comments as brief context in the top-level summary when helpful, but re-evaluate the current code on its own merits.\n\n");
        }

        if let Some(default_review_prompt) = self.review.default_review_prompt.as_deref() {
            prompt.push_str("## Default review prompt\n\n");
            prompt.push_str(default_review_prompt);
            prompt.push_str("\n\n");
        }

        if let Some(extra_instructions) = self.review.extra_instructions.as_deref() {
            prompt.push_str("## Extra instructions\n\n");
            prompt.push_str(extra_instructions);
            prompt.push_str("\n\n");
        }

        prompt.push_str("## Final response\n\n");
        prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

        prompt
    }
}

#[cfg(test)]
mod tests {
    use track_types::ids::{DispatchId, ProjectId, ReviewId};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord,
    };

    use super::RemoteReviewPrompt;

    fn parse_project_id(value: &str) -> ProjectId {
        ProjectId::new(value).expect("test project ids should be valid")
    }

    fn parse_review_id(value: &str) -> ReviewId {
        ReviewId::new(value).expect("test review ids should be valid")
    }

    fn parse_dispatch_id(value: &str) -> DispatchId {
        DispatchId::new(value).expect("test dispatch ids should be valid")
    }

    fn sample_review_record() -> ReviewRecord {
        let created_at = now_utc();

        ReviewRecord {
            id: parse_review_id("20260326-120000-review-pr-42"),
            pull_request_url: "https://github.com/acme/project-x/pull/42".to_owned(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-x".to_owned(),
            repo_url: "https://github.com/acme/project-x".to_owned(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            workspace_key: "project-x".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: Some(parse_project_id("project-x")),
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
        let previous_review_run = ReviewRunRecord {
            dispatch_id: parse_dispatch_id("review-dispatch-1"),
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
            branch_name: Some("track-review/review-dispatch-1".to_owned()),
            worktree_path: Some(
                "~/workspace/project-x/review-worktrees/review-dispatch-1".to_owned(),
            ),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: Some("Submitted a GitHub review with two inline comments.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(
                "https://github.com/acme/project-x/pull/42#pullrequestreview-1001".to_owned(),
            ),
            notes: None,
            error_message: None,
        };
        let current_review_run = ReviewRunRecord {
            dispatch_id: parse_dispatch_id("review-dispatch-2"),
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
            branch_name: Some("track-review/review-dispatch-2".to_owned()),
            worktree_path: Some(
                "~/workspace/project-x/review-worktrees/review-dispatch-2".to_owned(),
            ),
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

        assert!(prompt.contains("You are responsible for submitting the GitHub review yourself"));
        assert!(prompt.contains("Submit one GitHub review in COMMENT mode."));
        assert!(prompt.contains("Prefer inline review comments"));
        assert!(prompt.contains(
            "The first line of the top-level review body must be `@octocat requested me to review this PR.`"
        ));
        assert!(prompt.contains("- Pinned review commit: fedcba654321"));
        assert!(prompt.contains("the prepared worktree is intended to match that exact commit"));
        assert!(prompt.contains("Capture the submitted GitHub review's durable handle"));
        assert!(prompt.contains("Return `reviewSubmitted` as `true` only after GitHub confirms"));
        assert!(prompt.contains("## Current re-review request"));
        assert!(prompt.contains("Check whether the main review comments were actually resolved."));
        assert!(prompt.contains("## Previous bot review context"));
        assert!(prompt.contains("https://github.com/acme/project-x/pull/42#pullrequestreview-1001"));
        assert!(prompt.contains("## Re-review guidance"));
        assert!(prompt.contains(
            "non-blocking input at the discretion of the reviewee unless @octocat explicitly commented"
        ));
        assert!(prompt.contains("do not repeat it as a primary finding"));
        assert!(prompt.contains("## Default review prompt"));
        assert!(prompt.contains("Focus on regressions and missing tests."));
        assert!(prompt.contains("## Extra instructions"));
        assert!(prompt.contains("Pay special attention to queue rendering."));
    }
}
