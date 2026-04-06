use track_types::errors::{ErrorCode, TrackError};
use track_types::remote_layout::DispatchRunDirectory;
use track_types::types::{ReviewRunRecord, TaskDispatchRecord};

use track_config::runtime::RemoteAgentRuntimeConfig;

pub(crate) fn unique_review_worktree_paths(dispatch_history: &[ReviewRunRecord]) -> Vec<String> {
    dispatch_history
        .iter()
        .filter_map(|record| record.worktree_path.as_ref())
        .map(|path| path.clone().into_inner())
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
            if let Some(worktree_path) = record.worktree_path.as_ref() {
                return Some(worktree_path.run_directory().into_inner());
            }

            Some(
                DispatchRunDirectory::for_review(
                    &remote_agent.workspace_root,
                    &record.workspace_key,
                    &record.dispatch_id,
                )
                .into_inner(),
            )
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
    use track_types::ids::{DispatchId, ProjectId, ReviewId, TaskId};
    use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, WorkspaceKey};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, ReviewRunRecord, TaskDispatchRecord,
    };

    use crate::types::{GithubPullRequestMetadata, GithubPullRequestReference};

    use super::{describe_remote_reset_blockers, parse_github_repository_name};

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
            GithubPullRequestReference::parse("https://github.com/acme/project-x/pull/42")
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

        assert_eq!(metadata.workspace_key(), "acme-project-x");
    }

    #[test]
    fn reset_blockers_include_active_review_runs() {
        let created_at = now_utc();
        let task_dispatch_id = DispatchId::new("dispatch-1").unwrap();
        let task_project = ProjectId::new("project-a").unwrap();
        let task_dispatch = TaskDispatchRecord {
            dispatch_id: task_dispatch_id.clone(),
            task_id: TaskId::new("task-1").unwrap(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: task_project.clone(),
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some(DispatchBranch::for_task(&task_dispatch_id)),
            worktree_path: Some(DispatchWorktreePath::for_task(
                "~/workspace",
                &task_project,
                &task_dispatch_id,
            )),
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        let review_dispatch_id = DispatchId::new("review-dispatch-1").unwrap();
        let workspace_key = WorkspaceKey::new("project-a").unwrap();
        let review_dispatch = ReviewRunRecord {
            dispatch_id: review_dispatch_id.clone(),
            review_id: ReviewId::new("review-1").unwrap(),
            pull_request_url: "https://github.com/acme/project-a/pull/42".to_owned(),
            repository_full_name: "acme/project-a".to_owned(),
            workspace_key: workspace_key.clone(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            status: DispatchStatus::Running,
            created_at,
            updated_at: created_at,
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: Some(DispatchBranch::for_review(&review_dispatch_id)),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &workspace_key,
                &review_dispatch_id,
            )),
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
