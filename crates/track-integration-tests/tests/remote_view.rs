use std::time::Duration;

use serde_json::{json, Value};
use track_integration_tests::api_harness::ApiHarness;
use track_integration_tests::fixture::RemoteFixture;
use track_integration_tests::{
    live_integration_tests_enabled, print_live_test_skip_message, workspace_root,
};
use track_projects::project_metadata::ProjectMetadata;
use track_remote_agent::{RemoteRunObservedStatus, RemoteWorkspaceView, RemoteWorktreeKind};
use track_types::git_remote::GitRemote;
use track_types::ids::ProjectId;
use track_types::urls::Url;

// =============================================================================
// Live Remote-View Coverage
// =============================================================================
//
// This test exercises the new read facade against the same fixture-backed SSH
// environment as the dispatch integration tests. The goal is not exhaustive
// coverage; it is to prove that the view can reconstruct the main remote
// workspace slices we plan to build on later.
#[tokio::test(flavor = "multi_thread")]
async fn remote_view_loads_projects_dispatches_reviews_and_worktrees() {
    if !live_integration_tests_enabled() {
        print_live_test_skip_message();
        return;
    }

    let fixture = RemoteFixture::start(&workspace_root());
    fixture.seed_repo(
        "https://github.com/acme/project-a",
        "main",
        "fixture-user",
        "fixture-user",
    );
    fixture.write_codex_state(&success_codex_state(
        0,
        Some("https://github.com/acme/project-a/pull/42"),
        true,
        "Mock Codex completed the task and opened a PR.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Prepare the remote-view integration test fixture",
        )
        .await;

    let dispatch_response = harness.dispatch_task(&task.id).await;
    let task_dispatch_id = dispatch_response["dispatchId"]
        .as_str()
        .expect("dispatch response should include a dispatch id")
        .to_owned();
    let terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(terminal_dispatch["status"], "succeeded");

    fixture.write_codex_state(&review_codex_state(
        0,
        "Mock Codex reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nI did not find blocking issues in the fixture diff.",
    ));
    let review_response = harness
        .create_review(
            "https://github.com/acme/project-a/pull/42",
            Some("Pay extra attention to missing regression coverage."),
        )
        .await;
    let review_id = review_response["review"]["id"]
        .as_str()
        .expect("review response should include a review id")
        .to_owned();
    let review_dispatch_id = review_response["run"]["dispatchId"]
        .as_str()
        .expect("review response should include a dispatch id")
        .to_owned();
    let terminal_review_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(terminal_review_run["status"], "succeeded");

    let view = RemoteWorkspaceView::new(
        harness.remote_agent_runtime_config().await,
        harness.database(),
    )
    .expect("remote workspace view should construct");
    let project_id = ProjectId::new("project-a").expect("project id should be valid");

    assert_eq!(
        view.projects()
            .resolve_checkout_path_for_project(&project_id),
        "/home/track/workspace/project-a/project-a"
    );

    let task_dispatches = view
        .task_runs()
        .load_dispatch_views_for_project(&project_id)
        .await
        .expect("task dispatch views should load");
    assert_eq!(task_dispatches.len(), 1);
    assert_eq!(task_dispatches[0].record.dispatch_id, task_dispatch_id);
    assert_eq!(
        task_dispatches[0].remote.status,
        RemoteRunObservedStatus::Completed
    );
    let task_result = serde_json::from_str::<Value>(
        task_dispatches[0]
            .remote
            .result
            .as_deref()
            .expect("task dispatch view should include a structured result"),
    )
    .expect("task dispatch result should be valid JSON");
    assert_eq!(task_result["status"], "succeeded");

    let reviews = view
        .load_project_snapshot(&project_id)
        .await
        .expect("project snapshot should load");
    assert_eq!(reviews.project.canonical_name, "project-a");
    assert_eq!(reviews.task_dispatches.len(), 1);
    assert_eq!(reviews.reviews.len(), 1);
    assert_eq!(reviews.reviews[0].id, review_id);
    assert_eq!(reviews.review_runs.len(), 1);
    assert_eq!(
        reviews.review_runs[0].record.dispatch_id,
        review_dispatch_id
    );
    assert_eq!(
        reviews.review_runs[0].remote.status,
        RemoteRunObservedStatus::Completed
    );
    assert!(reviews.task_worktrees.iter().any(|entry| {
        entry.kind == RemoteWorktreeKind::Task
            && entry.path.as_str()
                == terminal_dispatch["worktreePath"]
                    .as_str()
                    .expect("task dispatch should include worktreePath")
    }));
    assert!(reviews.review_worktrees.iter().any(|entry| {
        entry.kind == RemoteWorktreeKind::Review
            && entry.path.as_str()
                == terminal_review_run["worktreePath"]
                    .as_str()
                    .expect("review run should include worktreePath")
    }));

    let task_run_directories = view
        .task_runs()
        .list_run_directories_for_project(&project_id)
        .await
        .expect("task run directories should load");
    assert!(task_run_directories.iter().any(|directory| directory
        .as_str()
        .ends_with(&format!("/dispatches/{task_dispatch_id}"))));

    let review_run_directories = view
        .review_runs()
        .list_run_directories_for_project(&project_id)
        .await
        .expect("review run directories should load");
    assert!(review_run_directories.iter().any(|directory| directory
        .as_str()
        .ends_with(&format!("/review-runs/{review_dispatch_id}"))));
}

fn project_metadata(project_name: &str) -> ProjectMetadata {
    ProjectMetadata {
        repo_url: Url::parse(&format!("https://github.com/acme/{project_name}")).unwrap(),
        git_url: GitRemote::new(&format!(
            "/srv/track-testing/git/upstream/{project_name}.git"
        ))
        .unwrap(),
        base_branch: "main".to_owned(),
        description: Some("Fixture-backed project metadata.".to_owned()),
    }
}

fn success_codex_state(
    sleep_seconds: u64,
    pull_request_url: Option<&str>,
    create_commit: bool,
    summary: &str,
) -> Value {
    json!({
        "mode": "success",
        "sleepSeconds": sleep_seconds,
        "status": "succeeded",
        "summary": summary,
        "pullRequestUrl": pull_request_url,
        "branchName": null,
        "worktreePath": null,
        "notes": "Recorded by the integration-test fixture.",
        "createCommit": if create_commit {
            json!({
                "message": "fix: apply mocked codex change",
                "files": [
                    {
                        "path": "MOCK_CODEX_CHANGE.md",
                        "contents": "# Mock change\n\nThis file was created by the Codex fixture.\n"
                    }
                ]
            })
        } else {
            Value::Null
        }
    })
}

fn review_codex_state(sleep_seconds: u64, summary: &str, review_body: &str) -> Value {
    json!({
        "mode": "success",
        "sleepSeconds": sleep_seconds,
        "status": "succeeded",
        "summary": summary,
        "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
        "reviewSubmitted": true,
        "reviewBody": review_body,
        "worktreePath": null,
        "notes": "Recorded by the integration-test fixture.",
        "createCommit": Value::Null,
    })
}
