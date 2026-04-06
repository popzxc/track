use std::path::PathBuf;

use tempfile::TempDir;
use track_projects::project_metadata::ProjectMetadata;
use track_types::git_remote::GitRemote;
use track_types::ids::{DispatchId, ProjectId, ReviewId, TaskId};
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, WorkspaceKey};
use track_types::time_utils::parse_iso_8601_millis;
use track_types::types::{
    DispatchStatus, Priority, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord, Status,
    Task, TaskDispatchRecord, TaskSource,
};
use track_types::urls::Url;

// =============================================================================
// Repository Test Fixtures
// =============================================================================
//
// The DAL tests need stable records with explicit timestamps so query ordering
// assertions do not accidentally depend on wall-clock time. Keeping those
// builders in one place lets each repository test focus on the query behavior
// it is trying to lock down.

pub(crate) fn temporary_database_path() -> (TempDir, PathBuf) {
    let directory = TempDir::new().expect("tempdir should be created");
    let database_path = directory.path().join("track.sqlite");
    (directory, database_path)
}

pub(crate) fn project_metadata(name: &str) -> ProjectMetadata {
    ProjectMetadata {
        repo_url: Url::parse(&format!("https://github.com/acme/{name}")).unwrap(),
        git_url: GitRemote::new(&format!("git@github.com:acme/{name}.git")).unwrap(),
        base_branch: "main".to_owned(),
        description: Some(format!("Metadata for {name}")),
    }
}

pub(crate) fn sample_task(
    id: &str,
    project: &str,
    priority: Priority,
    status: Status,
    description: &str,
    created_at: &str,
    updated_at: &str,
    source: Option<TaskSource>,
) -> Task {
    Task {
        id: TaskId::new(id).unwrap(),
        project: ProjectId::new(project).unwrap(),
        priority,
        status,
        description: description.to_owned(),
        created_at: parse_iso_8601_millis(created_at).expect("fixture created_at should parse"),
        updated_at: parse_iso_8601_millis(updated_at).expect("fixture updated_at should parse"),
        source,
    }
}

pub(crate) fn sample_dispatch(
    dispatch_id: &str,
    task_id: &str,
    project: &str,
    preferred_tool: RemoteAgentPreferredTool,
    status: DispatchStatus,
    created_at: &str,
    updated_at: &str,
) -> TaskDispatchRecord {
    let dispatch_id = DispatchId::new(dispatch_id).unwrap();
    let task_id = TaskId::new(task_id).unwrap();
    let project = ProjectId::new(project).unwrap();

    TaskDispatchRecord {
        dispatch_id: dispatch_id.clone(),
        task_id,
        preferred_tool,
        project: project.clone(),
        status,
        created_at: parse_iso_8601_millis(created_at).expect("fixture created_at should parse"),
        updated_at: parse_iso_8601_millis(updated_at).expect("fixture updated_at should parse"),
        finished_at: None,
        remote_host: "198.51.100.10".to_owned(),
        branch_name: Some(DispatchBranch::for_task(&dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_task(
            "/tmp",
            &project,
            &dispatch_id,
        )),
        pull_request_url: None,
        follow_up_request: None,
        summary: None,
        notes: None,
        error_message: None,
        review_request_head_oid: None,
        review_request_user: None,
    }
}

pub(crate) fn sample_review(
    id: &str,
    pull_request_number: u64,
    preferred_tool: RemoteAgentPreferredTool,
    created_at: &str,
    updated_at: &str,
) -> ReviewRecord {
    ReviewRecord {
        id: ReviewId::new(id).unwrap(),
        pull_request_url: Url::parse(&format!(
            "https://github.com/acme/project-a/pull/{pull_request_number}"
        ))
        .unwrap(),
        pull_request_number,
        pull_request_title: format!("Review {pull_request_number}"),
        repository_full_name: "acme/project-a".to_owned(),
        repo_url: Url::parse("https://github.com/acme/project-a").unwrap(),
        git_url: GitRemote::new("git@github.com:acme/project-a.git").unwrap(),
        base_branch: "main".to_owned(),
        workspace_key: WorkspaceKey::new("project-a").unwrap(),
        preferred_tool,
        project: Some(ProjectId::new("project-a").unwrap()),
        main_user: "octocat".to_owned(),
        default_review_prompt: Some("Focus on regressions.".to_owned()),
        extra_instructions: Some("Keep an eye on migrations.".to_owned()),
        created_at: parse_iso_8601_millis(created_at).expect("fixture created_at should parse"),
        updated_at: parse_iso_8601_millis(updated_at).expect("fixture updated_at should parse"),
    }
}

pub(crate) fn sample_review_run(
    dispatch_id: &str,
    review: &ReviewRecord,
    preferred_tool: RemoteAgentPreferredTool,
    status: DispatchStatus,
    created_at: &str,
    updated_at: &str,
) -> ReviewRunRecord {
    let dispatch_id = DispatchId::new(dispatch_id).unwrap();

    ReviewRunRecord {
        dispatch_id: dispatch_id.clone(),
        review_id: review.id.clone(),
        pull_request_url: review.pull_request_url.clone(),
        repository_full_name: review.repository_full_name.clone(),
        workspace_key: review.workspace_key.clone(),
        preferred_tool,
        status,
        created_at: parse_iso_8601_millis(created_at).expect("fixture created_at should parse"),
        updated_at: parse_iso_8601_millis(updated_at).expect("fixture updated_at should parse"),
        finished_at: None,
        remote_host: "198.51.100.10".to_owned(),
        branch_name: Some(DispatchBranch::for_review(&dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_review(
            "/tmp",
            &review.workspace_key,
            &dispatch_id,
        )),
        follow_up_request: None,
        target_head_oid: None,
        summary: None,
        review_submitted: false,
        github_review_id: None,
        github_review_url: None,
        notes: None,
        error_message: None,
    }
}
