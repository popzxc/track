use serde_json::json;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::task_description::parse_task_description;
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{ReviewRecord, ReviewRunRecord, TaskDispatchRecord};

use crate::constants::REVIEW_WORKTREE_DIRECTORY_NAME;
use crate::types::{
    GithubPullRequestMetadata, GithubPullRequestReference, GithubPullRequestReviewState,
    RemoteReviewFollowUpEvent,
};
use track_config::runtime::RemoteAgentRuntimeConfig;

pub(crate) fn unique_review_worktree_paths(dispatch_history: &[ReviewRunRecord]) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| record.worktree_path.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn unique_review_run_directories(
    dispatch_history: &[ReviewRunRecord],
    remote_agent: &RemoteAgentRuntimeConfig,
) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| {
            if let Some(worktree_path) = record.worktree_path.as_deref() {
                if let Some((prefix, _suffix)) =
                    worktree_path.rsplit_once(&format!("/{REVIEW_WORKTREE_DIRECTORY_NAME}/"))
                {
                    return Some(format!(
                        "{prefix}/{}/{}",
                        crate::constants::REVIEW_RUN_DIRECTORY_NAME,
                        record.dispatch_id
                    ));
                }
            }

            if record.workspace_key.trim().is_empty()
                || remote_agent.workspace_root.trim().is_empty()
            {
                return None;
            }

            Some(format!(
                "{}/{}/{}/{}",
                remote_agent.workspace_root.trim_end_matches('/'),
                record.workspace_key,
                crate::constants::REVIEW_RUN_DIRECTORY_NAME,
                record.dispatch_id
            ))
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn describe_remote_reset_blockers(
    task_dispatches: &[TaskDispatchRecord],
    review_dispatches: &[ReviewRunRecord],
) -> Vec<String> {
    let mut blockers = task_dispatches
        .iter()
        .filter(|record| record.status.is_active())
        .map(|record| format!("task {} ({})", record.task_id, record.dispatch_id))
        .collect::<Vec<_>>();
    blockers.extend(
        review_dispatches
            .iter()
            .filter(|record| record.status.is_active())
            .map(|record| format!("review {} ({})", record.review_id, record.dispatch_id)),
    );
    blockers
}

pub(crate) fn build_remote_dispatch_prompt(
    project_name: &str,
    metadata: &ProjectMetadata,
    branch_name: &str,
    worktree_path: &str,
    task_description: &str,
    pull_request_url: Option<&str>,
    follow_up_request: Option<&str>,
) -> String {
    let sections = parse_task_description(task_description);
    let mut prompt = String::new();
    prompt.push_str("# Remote task dispatch\n\n");
    prompt.push_str(
        "You are working in a fully autonomous mode on a prepared repository worktree.\n",
    );
    prompt.push_str("The repository checkout, fork, and worktree are already set up for you.\n");
    prompt.push_str("You have full filesystem access, internet access, and `gh` is available.\n");
    prompt.push_str("Make the decisions needed to complete the task responsibly.\n");
    prompt.push_str(
        "The desired outcome is a GitHub PR unless the task is blocked or cannot be solved.\n\n",
    );
    prompt.push_str("## Repository context\n\n");
    prompt.push_str(&format!("- Project: {project_name}\n"));
    prompt.push_str(&format!("- Repo URL: {}\n", metadata.repo_url));
    prompt.push_str(&format!("- Git URL: {}\n", metadata.git_url));
    prompt.push_str(&format!("- Base branch: {}\n", metadata.base_branch));
    prompt.push_str(&format!("- Prepared branch: {branch_name}\n"));
    prompt.push_str(&format!("- Working directory: {worktree_path}\n\n"));

    if let Some(pull_request_url) = pull_request_url.filter(|value| !value.trim().is_empty()) {
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
    prompt.push_str("- If the task is blocked, explain the blocker clearly in the final JSON.\n\n");
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

    if let Some(follow_up_request) = follow_up_request.filter(|value| !value.trim().is_empty()) {
        prompt.push_str("## Current follow-up request\n\n");
        prompt.push_str(follow_up_request.trim());
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Final response\n\n");
    prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

    prompt
}

pub(crate) fn build_remote_review_prompt(
    review: &ReviewRecord,
    dispatch_record: &ReviewRunRecord,
    previous_submitted_review: Option<&ReviewRunRecord>,
) -> String {
    let branch_name = dispatch_record
        .branch_name
        .as_deref()
        .expect("queued review dispatches should always have a branch name");
    let worktree_path = dispatch_record
        .worktree_path
        .as_deref()
        .expect("queued review dispatches should always have a worktree path");
    let mut prompt = String::new();
    prompt.push_str("# Remote PR review\n\n");
    prompt.push_str(
        "You are reviewing an existing GitHub pull request from a prepared repository worktree.\n",
    );
    prompt.push_str("The repository checkout and review worktree are already prepared for you.\n");
    prompt.push_str("You have full filesystem access, internet access, and `gh` is available.\n");
    prompt.push_str("This run is for review only: do not push commits, open PRs, or request reviewers yourself.\n");
    prompt.push_str("You are responsible for submitting the GitHub review yourself before you return the final JSON.\n\n");
    prompt.push_str("## Pull request context\n\n");
    prompt.push_str(&format!("- Pull request: {}\n", review.pull_request_url));
    prompt.push_str(&format!("- Title: {}\n", review.pull_request_title));
    prompt.push_str(&format!("- Repository: {}\n", review.repository_full_name));
    prompt.push_str(&format!("- Repo URL: {}\n", review.repo_url));
    prompt.push_str(&format!("- Base branch: {}\n", review.base_branch));
    prompt.push_str(&format!("- Prepared branch: {branch_name}\n"));
    prompt.push_str(&format!("- Working directory: {worktree_path}\n"));
    if let Some(target_head_oid) = dispatch_record.target_head_oid.as_deref() {
        prompt.push_str(&format!("- Pinned review commit: {target_head_oid}\n"));
    }
    prompt.push('\n');
    prompt.push_str("## Review instructions\n\n");
    prompt.push_str("- Submit one GitHub review in COMMENT mode.\n");
    prompt.push_str(&format!(
        "- The first line of the top-level review body must be `@{} requested me to review this PR.`\n",
        review.main_user
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

    if let Some(follow_up_request) = dispatch_record.follow_up_request.as_deref() {
        prompt.push_str("## Current re-review request\n\n");
        prompt.push_str(follow_up_request.trim());
        prompt.push_str("\n\n");
    }

    if let Some(previous_submitted_review) = previous_submitted_review {
        prompt.push_str("## Previous bot review context\n\n");
        if let Some(github_review_url) = previous_submitted_review.github_review_url.as_deref() {
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
            review.main_user
        ));
        prompt.push_str(&format!(
            "- Only treat an older bot finding as something you must actively verify and potentially elevate into a primary finding if @{} explicitly said it is valid and should be fixed.\n",
            review.main_user
        ));
        prompt.push_str(&format!(
            "- If @{} or the reviewee explicitly said an older bot finding is not important, disputed it, or chose not to address it, do not repeat it as a primary finding just because it appeared in a previous bot review.\n",
            review.main_user
        ));
        prompt.push_str("- You may mention unresolved prior bot comments as brief context in the top-level summary when helpful, but re-evaluate the current code on its own merits.\n\n");
    }

    if let Some(default_review_prompt) = review.default_review_prompt.as_deref() {
        prompt.push_str("## Default review prompt\n\n");
        prompt.push_str(default_review_prompt);
        prompt.push_str("\n\n");
    }

    if let Some(extra_instructions) = review.extra_instructions.as_deref() {
        prompt.push_str("## Extra instructions\n\n");
        prompt.push_str(extra_instructions);
        prompt.push_str("\n\n");
    }

    prompt.push_str("## Final response\n\n");
    prompt.push_str("Return JSON only. The response must match the provided schema exactly.\n");

    prompt
}

pub(crate) fn build_remote_dispatch_schema() -> String {
    serde_json::to_string_pretty(&json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "status",
            "summary",
            "pullRequestUrl",
            "branchName",
            "worktreePath",
            "notes"
        ],
        "properties": {
            "status": {
                "type": "string",
                "enum": ["succeeded", "failed", "blocked"]
            },
            "summary": {
                "type": "string"
            },
            "pullRequestUrl": {
                "type": ["string", "null"]
            },
            "branchName": {
                "type": ["string", "null"]
            },
            "worktreePath": {
                "type": "string"
            },
            "notes": {
                "type": ["string", "null"]
            }
        }
    }))
    .expect("dispatch schema serialization should succeed")
}

pub(crate) fn build_remote_review_schema() -> String {
    serde_json::to_string_pretty(&json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "status",
            "summary",
            "reviewSubmitted",
            "githubReviewId",
            "githubReviewUrl",
            "worktreePath",
            "notes"
        ],
        "properties": {
            "status": {
                "type": "string",
                "enum": ["succeeded", "failed", "blocked"]
            },
            "summary": {
                "type": "string"
            },
            "reviewSubmitted": {
                "type": "boolean"
            },
            "githubReviewId": {
                "type": ["string", "null"]
            },
            "githubReviewUrl": {
                "type": ["string", "null"]
            },
            "worktreePath": {
                "type": "string"
            },
            "notes": {
                "type": ["string", "null"]
            }
        }
    }))
    .expect("review schema serialization should succeed")
}

pub(crate) fn parse_github_repository_name(repo_url: &str) -> Result<String, TrackError> {
    let trimmed = repo_url.trim().trim_end_matches('/');
    let without_suffix = trimmed.trim_end_matches(".git");
    let Some(repository_name) = without_suffix.rsplit('/').next() else {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Repo URL {repo_url} does not look like a GitHub repository."),
        ));
    };

    if !without_suffix.contains("github.com/") || repository_name.is_empty() {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Repo URL {repo_url} does not look like a GitHub repository."),
        ));
    }

    Ok(repository_name.to_owned())
}

pub(crate) fn parse_github_pull_request_reference(
    pull_request_url: &str,
) -> Result<GithubPullRequestReference, TrackError> {
    let trimmed = pull_request_url.trim().trim_end_matches('/');
    let without_scheme = trimmed.strip_prefix("https://github.com/").ok_or_else(|| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!(
                "Pull request URL {pull_request_url} does not look like a GitHub pull request."
            ),
        )
    })?;
    let parts = without_scheme.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" {
        return Err(TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!(
                "Pull request URL {pull_request_url} does not look like a GitHub pull request."
            ),
        ));
    }

    let number = parts[3].parse::<u64>().map_err(|_| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!("Pull request URL {pull_request_url} does not contain a valid PR number."),
        )
    })?;

    Ok(GithubPullRequestReference {
        owner: parts[0].to_owned(),
        repository: parts[1].to_owned(),
        number,
    })
}

pub(crate) fn build_review_workspace_key(pull_request: &GithubPullRequestMetadata) -> String {
    let slug = slug::slugify(pull_request.repository_full_name.replace('/', "-").trim());

    if slug.is_empty() {
        "review-repo".to_owned()
    } else {
        slug
    }
}

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

pub(crate) fn build_review_follow_up_notification_comment(
    main_user: &str,
    head_oid: &str,
) -> String {
    let short_head_oid = head_oid.get(..7).unwrap_or(head_oid);

    format!(
        "@{main_user} new bot updates are ready on commit `{short_head_oid}`. \
Please leave a PR review (COMMENTED or CHANGES_REQUESTED) if you want the bot to follow up automatically."
    )
}

pub(crate) fn review_follow_up_event(
    outcome: &str,
    detail: impl Into<String>,
    dispatch_record: &TaskDispatchRecord,
    reviewer: &str,
    pull_request_state: Option<&GithubPullRequestReviewState>,
) -> RemoteReviewFollowUpEvent {
    let latest_review_state = pull_request_state
        .and_then(|state| state.latest_eligible_review.as_ref())
        .map(|review| review.state.clone());
    let latest_review_submitted_at = pull_request_state
        .and_then(|state| state.latest_eligible_review.as_ref())
        .map(|review| format_iso_8601_millis(review.submitted_at));

    RemoteReviewFollowUpEvent {
        outcome: outcome.to_owned(),
        detail: detail.into(),
        task_id: dispatch_record.task_id.clone(),
        dispatch_id: dispatch_record.dispatch_id.clone(),
        dispatch_status: dispatch_record.status.as_str().to_owned(),
        remote_host: dispatch_record.remote_host.clone(),
        branch_name: dispatch_record.branch_name.clone(),
        pull_request_url: dispatch_record.pull_request_url.clone(),
        reviewer: reviewer.to_owned(),
        pr_is_open: pull_request_state.map(|state| state.is_open),
        pr_head_oid: pull_request_state.map(|state| state.head_oid.clone()),
        latest_review_state,
        latest_review_submitted_at,
    }
}

pub(crate) fn contextualize_track_error(
    error: TrackError,
    context: impl Into<String>,
) -> TrackError {
    TrackError::new(
        error.code,
        format!("{}: {}", context.into(), error.message()),
    )
}
