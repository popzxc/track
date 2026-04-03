use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{ReviewRunRecord, TaskDispatchRecord};

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

#[cfg(test)]
mod tests {
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, ReviewRunRecord, TaskDispatchRecord,
    };

    use super::{
        build_review_workspace_key, describe_remote_reset_blockers,
        parse_github_pull_request_reference, parse_github_repository_name,
        GithubPullRequestMetadata,
    };

    #[test]
    fn parses_github_repository_name() {
        assert_eq!(
            parse_github_repository_name("https://github.com/acme/project-x")
                .expect("github url should parse"),
            "project-x"
        );
    }

    #[test]
    fn parses_github_pull_request_reference() {
        let reference =
            parse_github_pull_request_reference("https://github.com/acme/project-x/pull/42")
                .expect("github pr url should parse");

        assert_eq!(reference.owner, "acme");
        assert_eq!(reference.repository, "project-x");
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn builds_review_workspace_key_from_repository_name() {
        let metadata = GithubPullRequestMetadata {
            pull_request_url: "https://github.com/acme/project-x/pull/42".to_owned(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-x".to_owned(),
            repo_url: "https://github.com/acme/project-x".to_owned(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            head_oid: "abc123".to_owned(),
        };

        assert_eq!(build_review_workspace_key(&metadata), "acme-project-x");
    }

    #[test]
    fn reset_blockers_include_active_review_runs() {
        let created_at = now_utc();
        let task_dispatch = TaskDispatchRecord {
            dispatch_id: "dispatch-1".to_owned(),
            task_id: "task-1".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: "project-a".to_owned(),
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some("track/dispatch-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/worktrees/dispatch-1".to_owned()),
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        let review_dispatch = ReviewRunRecord {
            dispatch_id: "review-dispatch-1".to_owned(),
            review_id: "review-1".to_owned(),
            pull_request_url: "https://github.com/acme/project-a/pull/42".to_owned(),
            repository_full_name: "acme/project-a".to_owned(),
            workspace_key: "project-a".to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some("track-review/review-dispatch-1".to_owned()),
            worktree_path: Some(
                "~/workspace/project-a/review-worktrees/review-dispatch-1".to_owned(),
            ),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: None,
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };

        let blockers = describe_remote_reset_blockers(&[task_dispatch], &[review_dispatch]);

        assert_eq!(blockers.len(), 2);
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains("task-1") && blocker.contains("task")));
        assert!(blockers
            .iter()
            .any(|blocker| blocker.contains("review-1") && blocker.contains("review")));
    }
}
