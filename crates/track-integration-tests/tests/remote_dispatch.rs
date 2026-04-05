use std::collections::BTreeSet;
use std::time::Duration;

use axum::http::StatusCode;
use serde_json::{json, Value};
use track_integration_tests::api_harness::ApiHarness;
use track_integration_tests::fixture::RemoteFixture;
use track_integration_tests::{
    live_integration_tests_enabled, print_live_test_skip_message, workspace_root,
};
use track_projects::project_metadata::ProjectMetadata;

// =============================================================================
// First Live Remote-Dispatch Test
// =============================================================================
//
// This test keeps the positive-path scope intentionally narrow. It verifies
// that the real API can prepare and launch a dispatch over SSH, that the remote
// fixture receives the expected files, and that the resulting dispatch is
// observed as succeeded through the normal refresh endpoint.
#[tokio::test(flavor = "multi_thread")]
async fn dispatch_task_reaches_succeeded_over_real_ssh() {
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
            "Prepare the remote-agent integration harness",
        )
        .await;

    let dispatch_response = harness.dispatch_task(&task.id).await;
    let dispatch_id = dispatch_response["dispatchId"]
        .as_str()
        .expect("dispatch response should include a dispatch id")
        .to_owned();
    assert_eq!(dispatch_response["status"], "preparing");

    let terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(
        terminal_dispatch["status"]
            .as_str()
            .expect("terminal status should be a string"),
        "succeeded"
    );
    assert_eq!(
        terminal_dispatch["pullRequestUrl"]
            .as_str()
            .expect("terminal dispatch should include pullRequestUrl"),
        "https://github.com/acme/project-a/pull/42"
    );
    assert_eq!(
        terminal_dispatch["branchName"]
            .as_str()
            .expect("terminal dispatch should include branchName"),
        format!("track/{dispatch_id}")
    );
    assert!(terminal_dispatch["worktreePath"]
        .as_str()
        .expect("terminal dispatch should include worktreePath")
        .ends_with(&format!("/project-a/worktrees/{dispatch_id}")));

    let remote_run_directory = format!("/home/track/workspace/project-a/dispatches/{dispatch_id}");
    let registry_contents =
        fixture.read_remote_file("/srv/track-testing/state/track-projects.json");
    assert!(registry_contents.contains("\"project-a\""));

    let remote_status = fixture.read_remote_file(&format!("{remote_run_directory}/status.txt"));
    assert_eq!(remote_status.trim(), "completed");

    let remote_result = fixture.read_remote_file(&format!("{remote_run_directory}/result.json"));
    assert!(remote_result.contains("\"status\": \"succeeded\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_task_uses_claude_when_runner_prefers_claude() {
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
    fixture.write_claude_state(&success_claude_state(
        0,
        Some("https://github.com/acme/project-a/pull/42"),
        true,
        "Mock Claude completed the task and opened a PR.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let updated_settings = harness
        .update_remote_agent_settings(json!({
            "preferredTool": "claude",
            "shellPrelude": "export PATH=\"/opt/track-testing/bin:$PATH\"\nexport TRACK_TESTING_RUNTIME_DIR=\"/srv/track-testing\"",
            "reviewFollowUp": {
                "enabled": false,
                "mainUser": "octocat",
                "defaultReviewPrompt": "Focus on bugs, regressions, and missing tests.",
            }
        }))
        .await;
    assert_eq!(updated_settings["preferredTool"], "claude");

    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Prepare the remote-agent integration harness with Claude",
        )
        .await;

    let dispatch_response = harness.dispatch_task(&task.id).await;
    let dispatch_id = dispatch_response["dispatchId"]
        .as_str()
        .expect("dispatch response should include a dispatch id")
        .to_owned();
    assert_eq!(dispatch_response["status"], "preparing");

    let terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(terminal_dispatch["status"], "succeeded");
    assert_eq!(
        terminal_dispatch["summary"],
        "Mock Claude completed the task and opened a PR."
    );
    assert_eq!(
        terminal_dispatch["pullRequestUrl"],
        "https://github.com/acme/project-a/pull/42"
    );

    let claude_log_entries = fixture.read_log_entries("claude");
    assert_eq!(claude_log_entries.len(), 1);
    assert!(claude_log_entries[0]["argv"]
        .as_array()
        .is_some_and(|argv| argv.iter().any(|value| value.as_str() == Some("-p"))));
    assert!(claude_log_entries[0]["argv"]
        .as_array()
        .is_some_and(|argv| {
            argv.iter()
                .any(|value| value.as_str() == Some("--dangerously-skip-permissions"))
        }));
    assert!(claude_log_entries[0]["argv"]
        .as_array()
        .is_some_and(|argv| {
            argv.windows(2).any(|window| {
                window[0].as_str() == Some("--output-format") && window[1].as_str() == Some("json")
            })
        }));
    assert!(fixture.read_log_entries("codex").is_empty());

    let remote_run_directory = format!("/home/track/workspace/project-a/dispatches/{dispatch_id}");
    let remote_result = fixture.read_remote_file(&format!("{remote_run_directory}/result.json"));
    assert!(remote_result.contains("\"structured_output\""));
    assert!(
        remote_result.contains("\"summary\": \"Mock Claude completed the task and opened a PR.\"")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dispatch_task_can_override_runner_tool_per_request() {
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
    fixture.write_claude_state(&success_claude_state(
        0,
        Some("https://github.com/acme/project-a/pull/42"),
        true,
        "Mock Claude completed the task through a per-request override.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Prepare the remote-agent integration harness with a per-request Claude override",
        )
        .await;

    let dispatch_response = harness
        .dispatch_task_with_tool(&task.id, Some("claude"))
        .await;
    assert_eq!(dispatch_response["preferredTool"], "claude");

    let terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(terminal_dispatch["status"], "succeeded");
    assert_eq!(terminal_dispatch["preferredTool"], "claude");
    assert_eq!(
        terminal_dispatch["summary"],
        "Mock Claude completed the task through a per-request override."
    );

    let claude_log_entries = fixture.read_log_entries("claude");
    assert_eq!(claude_log_entries.len(), 1);
    assert!(claude_log_entries[0]["argv"]
        .as_array()
        .is_some_and(|argv| {
            argv.windows(2).any(|window| {
                window[0].as_str() == Some("--output-format") && window[1].as_str() == Some("json")
            })
        }));
    assert!(fixture.read_log_entries("codex").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn follow_up_reuses_branch_worktree_and_existing_pr_context() {
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
        false,
        "Mock Codex completed the task and opened a PR.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Prepare the remote-agent integration harness",
        )
        .await;

    let first_dispatch = harness.dispatch_task(&task.id).await;
    let first_dispatch_id = first_dispatch["dispatchId"]
        .as_str()
        .expect("first dispatch response should include a dispatch id")
        .to_owned();
    let first_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(first_terminal_dispatch["status"], "succeeded");

    let follow_up_request = "Address the PR review comments and keep using the existing PR.";
    let second_dispatch = harness.follow_up_task(&task.id, follow_up_request).await;
    let second_dispatch_id = second_dispatch["dispatchId"]
        .as_str()
        .expect("follow-up response should include a dispatch id")
        .to_owned();
    assert_ne!(second_dispatch_id, first_dispatch_id);
    assert_eq!(
        second_dispatch["branchName"],
        first_terminal_dispatch["branchName"]
    );
    assert_eq!(
        second_dispatch["worktreePath"],
        first_terminal_dispatch["worktreePath"]
    );
    assert_eq!(
        second_dispatch["pullRequestUrl"],
        first_terminal_dispatch["pullRequestUrl"]
    );
    assert_eq!(second_dispatch["followUpRequest"], follow_up_request);

    let second_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(second_terminal_dispatch["status"], "succeeded");
    assert_eq!(
        second_terminal_dispatch["branchName"],
        first_terminal_dispatch["branchName"]
    );
    assert_eq!(
        second_terminal_dispatch["worktreePath"],
        first_terminal_dispatch["worktreePath"]
    );
    assert_eq!(
        second_terminal_dispatch["pullRequestUrl"],
        first_terminal_dispatch["pullRequestUrl"]
    );
    assert_eq!(
        second_terminal_dispatch["followUpRequest"],
        follow_up_request
    );

    let follow_up_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{second_dispatch_id}");
    let remote_prompt = fixture.read_remote_file(&format!("{follow_up_run_directory}/prompt.md"));
    assert!(remote_prompt.contains("## Existing PR"));
    assert!(remote_prompt.contains("https://github.com/acme/project-a/pull/42"));
    assert!(remote_prompt.contains("## Current follow-up request"));
    assert!(remote_prompt.contains(follow_up_request));

    let updated_task = harness.load_task(&task.id).await;
    assert!(updated_task.description.contains("## Follow-up requests"));
    assert!(updated_task.description.contains(follow_up_request));

    let remote_status = fixture.read_remote_file(&format!("{follow_up_run_directory}/status.txt"));
    assert_eq!(remote_status.trim(), "completed");
}

#[tokio::test(flavor = "multi_thread")]
async fn requesting_a_pr_review_posts_the_review_and_cleans_up_artifacts() {
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
    fixture.write_codex_state(&review_codex_state(
        0,
        "Mock Codex reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nI did not find blocking issues in the fixture diff.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let _metadata_seed_task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Seed project metadata for manual review requests",
        )
        .await;
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
    let dispatch_id = review_response["run"]["dispatchId"]
        .as_str()
        .expect("review response should include a dispatch id")
        .to_owned();

    let terminal_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(
        terminal_run["status"],
        "succeeded",
        "terminal review run: {}",
        serde_json::to_string_pretty(&terminal_run).expect("terminal review run should serialize")
    );
    assert_eq!(terminal_run["reviewSubmitted"], true);
    assert_eq!(
        terminal_run["summary"],
        "Mock Codex reviewed the pull request successfully."
    );
    assert_eq!(terminal_run["githubReviewId"], "42001");
    assert_eq!(
        terminal_run["githubReviewUrl"],
        "https://github.com/acme/project-a/pull/42#pullrequestreview-42001"
    );

    let review_worktree_path = terminal_run["worktreePath"]
        .as_str()
        .expect("review run should include a worktree path")
        .to_owned();
    let review_run_directory = format!("/home/track/workspace/project-a/review-runs/{dispatch_id}");
    let remote_prompt = fixture.read_remote_file(&format!("{review_run_directory}/prompt.md"));
    assert!(remote_prompt.contains("## Default review prompt"));
    assert!(remote_prompt.contains("## Extra instructions"));
    assert!(remote_prompt.contains("Pay extra attention to missing regression coverage."));

    let gh_log_entries = fixture.read_log_entries("gh");
    let review_post_entry = gh_log_entries
        .iter()
        .find(|entry| {
            entry["result"]["endpoint"]
                .as_str()
                .is_some_and(|endpoint| endpoint.ends_with("/pulls/42/reviews"))
        })
        .expect("gh log should include the posted PR review");
    assert!(review_post_entry["result"]["reviewBody"]
        .as_str()
        .expect("review post log should include the review body")
        .starts_with("@octocat requested me to review this PR."));
    assert_eq!(review_post_entry["result"]["reviewId"], "42001");
    assert_eq!(
        review_post_entry["result"]["reviewUrl"],
        "https://github.com/acme/project-a/pull/42#pullrequestreview-42001"
    );

    assert!(harness.review_record_exists(&review_id).await);
    assert!(harness.review_history_exists(&review_id).await);
    assert!(fixture.remote_path_exists(&review_worktree_path));
    assert!(fixture.remote_path_exists(&review_run_directory));

    harness.delete_review(&review_id).await;

    assert!(!harness.review_record_exists(&review_id).await);
    assert!(!harness.review_history_exists(&review_id).await);
    assert!(!fixture.remote_path_exists(&review_worktree_path));
    assert!(!fixture.remote_path_exists(&review_run_directory));
}

#[tokio::test(flavor = "multi_thread")]
async fn requesting_a_rereview_reuses_the_saved_review_and_records_new_run_context() {
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
    fixture.write_codex_state(&review_codex_state(
        0,
        "Mock Codex reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nI found one issue in the fixture diff.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let _metadata_seed_task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Seed project metadata for manual review requests",
        )
        .await;
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
    let initial_terminal_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(initial_terminal_run["status"], "succeeded");
    assert_eq!(initial_terminal_run["githubReviewId"], "42001");

    fixture.write_codex_state(&review_codex_state(
        0,
        "Mock Codex re-reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nThe previously endorsed issue looks fixed, and I did not find new blocking problems.",
    ));

    let follow_up_request =
        "Check whether the comments I confirmed are fixed, and mention anything intentionally left alone only in the summary.";
    let follow_up_response = harness
        .follow_up_review(&review_id, follow_up_request)
        .await;
    let follow_up_dispatch_id = follow_up_response["dispatchId"]
        .as_str()
        .expect("re-review response should include a dispatch id")
        .to_owned();
    assert_eq!(follow_up_response["reviewId"], review_id);
    assert_eq!(follow_up_response["followUpRequest"], follow_up_request);
    assert!(follow_up_response["targetHeadOid"]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty()));

    let follow_up_terminal_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(follow_up_terminal_run["status"], "succeeded");
    assert_eq!(follow_up_terminal_run["followUpRequest"], follow_up_request);
    assert_eq!(follow_up_terminal_run["githubReviewId"], "42002");
    assert_eq!(
        follow_up_terminal_run["githubReviewUrl"],
        "https://github.com/acme/project-a/pull/42#pullrequestreview-42002"
    );

    let review_runs = harness.review_runs(&review_id).await;
    assert_eq!(review_runs.len(), 2);
    assert_eq!(review_runs[0]["dispatchId"], follow_up_dispatch_id);
    assert_eq!(review_runs[1]["githubReviewId"], "42001");

    let follow_up_worktree_path = follow_up_terminal_run["worktreePath"]
        .as_str()
        .expect("re-review run should include a worktree path")
        .to_owned();
    let follow_up_run_directory =
        format!("/home/track/workspace/project-a/review-runs/{follow_up_dispatch_id}");
    let remote_prompt = fixture.read_remote_file(&format!("{follow_up_run_directory}/prompt.md"));
    assert!(remote_prompt.contains("## Current re-review request"));
    assert!(remote_prompt.contains(follow_up_request));
    assert!(remote_prompt.contains("## Previous bot review context"));
    assert!(
        remote_prompt.contains("https://github.com/acme/project-a/pull/42#pullrequestreview-42001")
    );
    assert!(remote_prompt.contains("## Re-review guidance"));
    assert!(remote_prompt.contains(
        "your previous comments are always non-blocking input at the discretion of the reviewee"
    ));
    assert!(remote_prompt.contains(
        "do not repeat it as a primary finding just because it appeared in a previous bot review"
    ));

    let gh_log_entries = fixture.read_log_entries("gh");
    let posted_review_ids = gh_log_entries
        .iter()
        .filter_map(|entry| {
            entry["result"]["endpoint"]
                .as_str()
                .filter(|endpoint| endpoint.ends_with("/pulls/42/reviews"))
                .map(|_| {
                    entry["result"]["reviewId"]
                        .as_str()
                        .expect("review post log should include the review id")
                        .to_owned()
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        posted_review_ids,
        vec!["42001".to_owned(), "42002".to_owned()]
    );

    harness.delete_review(&review_id).await;

    assert!(!harness.review_record_exists(&review_id).await);
    assert!(!harness.review_history_exists(&review_id).await);
    assert!(!fixture.remote_path_exists(&follow_up_worktree_path));
    assert!(!fixture.remote_path_exists(&follow_up_run_directory));
}

#[tokio::test(flavor = "multi_thread")]
async fn requesting_a_pr_review_can_override_runner_tool_and_rereview_keeps_it() {
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
    fixture.write_claude_state(&review_claude_state(
        0,
        "Mock Claude reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nI found one issue in the fixture diff.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let _metadata_seed_task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Seed project metadata for per-review runner overrides",
        )
        .await;
    let review_response = harness
        .create_review_with_tool(
            "https://github.com/acme/project-a/pull/42",
            Some("Pay extra attention to missing regression coverage."),
            Some("claude"),
        )
        .await;
    let review_id = review_response["review"]["id"]
        .as_str()
        .expect("review response should include a review id")
        .to_owned();
    assert_eq!(review_response["review"]["preferredTool"], "claude");
    assert_eq!(review_response["run"]["preferredTool"], "claude");

    let initial_terminal_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(initial_terminal_run["status"], "succeeded");
    assert_eq!(initial_terminal_run["preferredTool"], "claude");
    assert_eq!(initial_terminal_run["githubReviewId"], "42001");

    fixture.write_claude_state(&review_claude_state(
        0,
        "Mock Claude re-reviewed the pull request successfully.",
        "@octocat requested me to review this PR.\n\nThe previously endorsed issue looks fixed, and I did not find new blocking problems.",
    ));

    let follow_up_response = harness
        .follow_up_review(
            &review_id,
            "Check whether the comments I confirmed are fixed, and mention anything intentionally left alone only in the summary.",
        )
        .await;
    assert_eq!(follow_up_response["preferredTool"], "claude");

    let follow_up_terminal_run = harness
        .poll_review_until_terminal(&review_id, Duration::from_secs(20))
        .await;
    assert_eq!(follow_up_terminal_run["status"], "succeeded");
    assert_eq!(follow_up_terminal_run["preferredTool"], "claude");
    assert_eq!(follow_up_terminal_run["githubReviewId"], "42002");

    let review_runs = harness.review_runs(&review_id).await;
    assert_eq!(review_runs.len(), 2);
    assert!(review_runs
        .iter()
        .all(|run| run["preferredTool"] == "claude"));

    let claude_log_entries = fixture.read_log_entries("claude");
    assert_eq!(claude_log_entries.len(), 2);
    assert!(claude_log_entries.iter().all(|entry| {
        entry["argv"].as_array().is_some_and(|argv| {
            argv.windows(2).any(|window| {
                window[0].as_str() == Some("--output-format") && window[1].as_str() == Some("json")
            })
        })
    }));
    assert!(fixture.read_log_entries("codex").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_task_removes_local_and_remote_dispatch_artifacts() {
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
        false,
        "Mock Codex completed the task and opened a PR.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Delete the remote-agent artifacts with the task itself",
        )
        .await;

    let dispatch_response = harness.dispatch_task(&task.id).await;
    let dispatch_id = dispatch_response["dispatchId"]
        .as_str()
        .expect("dispatch response should include a dispatch id")
        .to_owned();
    let terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(terminal_dispatch["status"], "succeeded");

    let worktree_path = terminal_dispatch["worktreePath"]
        .as_str()
        .expect("terminal dispatch should include a worktree path")
        .to_owned();
    let remote_run_directory = format!("/home/track/workspace/project-a/dispatches/{dispatch_id}");
    assert!(fixture.remote_path_exists(&worktree_path));
    assert!(fixture.remote_path_exists(&remote_run_directory));
    assert!(harness.dispatch_history_exists(&task.id).await);

    harness.delete_task(&task.id).await;

    assert!(!harness.task_exists(&task.id).await);
    assert!(!harness.dispatch_history_exists(&task.id).await);
    assert!(!fixture.remote_path_exists(&worktree_path));
    assert!(!fixture.remote_path_exists(&remote_run_directory));
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_a_task_releases_the_worktree_but_keeps_history_for_reopen() {
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
        false,
        "Mock Codex completed the task and opened a PR.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Close the task, then recreate the worktree on reopen",
        )
        .await;

    let first_dispatch = harness.dispatch_task(&task.id).await;
    let first_dispatch_id = first_dispatch["dispatchId"]
        .as_str()
        .expect("first dispatch response should include a dispatch id")
        .to_owned();
    let first_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(first_terminal_dispatch["status"], "succeeded");

    let original_worktree_path = first_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("terminal dispatch should include a worktree path")
        .to_owned();
    let original_remote_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{first_dispatch_id}");
    assert!(fixture.remote_path_exists(&original_worktree_path));
    assert!(fixture.remote_path_exists(&original_remote_run_directory));
    assert!(harness.dispatch_history_exists(&task.id).await);

    let closed_task = harness.update_task_status(&task.id, "closed").await;
    assert_eq!(closed_task["status"], "closed");
    assert!(!fixture.remote_path_exists(&original_worktree_path));
    assert!(fixture.remote_path_exists(&original_remote_run_directory));
    assert!(harness.dispatch_history_exists(&task.id).await);

    let reopened_task = harness.update_task_status(&task.id, "open").await;
    assert_eq!(reopened_task["status"], "open");

    fixture.write_codex_state(&success_codex_state(
        0,
        Some("https://github.com/acme/project-a/pull/42"),
        false,
        "Mock Codex completed the follow-up after the worktree was restored.",
    ));

    let follow_up_dispatch = harness
        .follow_up_task(
            &task.id,
            "Continue from the existing PR after reopening the task.",
        )
        .await;
    let follow_up_dispatch_id = follow_up_dispatch["dispatchId"]
        .as_str()
        .expect("follow-up response should include a dispatch id")
        .to_owned();
    let second_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;

    assert_eq!(second_terminal_dispatch["status"], "succeeded");
    assert_eq!(
        second_terminal_dispatch["worktreePath"],
        first_terminal_dispatch["worktreePath"]
    );
    assert_eq!(
        second_terminal_dispatch["pullRequestUrl"],
        first_terminal_dispatch["pullRequestUrl"]
    );

    let follow_up_worktree_path = second_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("follow-up terminal dispatch should include a worktree path")
        .to_owned();
    let follow_up_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{follow_up_dispatch_id}");
    assert!(fixture.remote_path_exists(&follow_up_worktree_path));
    assert!(fixture.remote_path_exists(&follow_up_run_directory));
    assert!(fixture.remote_path_exists(&original_remote_run_directory));
}

#[tokio::test(flavor = "multi_thread")]
async fn manual_cleanup_reconciles_closed_and_missing_task_artifacts() {
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
        false,
        "Mock Codex completed the cleanup scenario.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let closed_task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Close this task later and keep only its metadata",
        )
        .await;
    let missing_task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Delete this task file and let manual cleanup remove everything",
        )
        .await;

    let closed_dispatch = harness.dispatch_task(&closed_task.id).await;
    let closed_dispatch_id = closed_dispatch["dispatchId"]
        .as_str()
        .expect("closed dispatch should include a dispatch id")
        .to_owned();
    let closed_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&closed_task.id, Duration::from_secs(20))
        .await;
    assert_eq!(closed_terminal_dispatch["status"], "succeeded");

    let missing_dispatch = harness.dispatch_task(&missing_task.id).await;
    let missing_dispatch_id = missing_dispatch["dispatchId"]
        .as_str()
        .expect("missing dispatch should include a dispatch id")
        .to_owned();
    let missing_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&missing_task.id, Duration::from_secs(20))
        .await;
    assert_eq!(missing_terminal_dispatch["status"], "succeeded");

    let closed_worktree_path = closed_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("closed task dispatch should include a worktree path")
        .to_owned();
    let missing_worktree_path = missing_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("missing task dispatch should include a worktree path")
        .to_owned();
    let closed_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{closed_dispatch_id}");
    let missing_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{missing_dispatch_id}");

    harness
        .close_task_without_remote_cleanup(&closed_task.id)
        .await;
    harness
        .delete_task_file_without_remote_cleanup(&missing_task.id)
        .await;

    assert!(fixture.remote_path_exists(&closed_worktree_path));
    assert!(fixture.remote_path_exists(&closed_run_directory));
    assert!(harness.dispatch_history_exists(&closed_task.id).await);
    assert!(fixture.remote_path_exists(&missing_worktree_path));
    assert!(fixture.remote_path_exists(&missing_run_directory));
    assert!(harness.dispatch_history_exists(&missing_task.id).await);

    let cleanup_response = harness.cleanup_remote_agent_artifacts().await;
    let summary = &cleanup_response["summary"];
    assert_eq!(summary["closedTasksCleaned"], 1);
    assert_eq!(summary["missingTasksCleaned"], 1);
    assert_eq!(summary["localDispatchHistoriesRemoved"], 1);
    assert_eq!(summary["remoteWorktreesRemoved"], 2);
    assert_eq!(summary["remoteRunDirectoriesRemoved"], 1);

    assert!(harness.task_exists(&closed_task.id).await);
    assert!(!fixture.remote_path_exists(&closed_worktree_path));
    assert!(fixture.remote_path_exists(&closed_run_directory));
    assert!(harness.dispatch_history_exists(&closed_task.id).await);

    assert!(!harness.task_exists(&missing_task.id).await);
    assert!(!fixture.remote_path_exists(&missing_worktree_path));
    assert!(!fixture.remote_path_exists(&missing_run_directory));
    assert!(!harness.dispatch_history_exists(&missing_task.id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn resetting_remote_workspace_rebuilds_cleanly_from_local_tracker_state() {
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
        false,
        "Mock Codex completed the initial reset scenario.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Rebuild the remote workspace after a manual reset",
        )
        .await;

    let first_dispatch = harness.dispatch_task(&task.id).await;
    let first_dispatch_id = first_dispatch["dispatchId"]
        .as_str()
        .expect("first dispatch response should include a dispatch id")
        .to_owned();
    let first_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(first_terminal_dispatch["status"], "succeeded");

    let original_worktree_path = first_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("first terminal dispatch should include a worktree path")
        .to_owned();
    let original_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{first_dispatch_id}");
    assert!(fixture.remote_path_exists("/home/track/workspace/project-a"));
    assert!(fixture.remote_path_exists(&original_worktree_path));
    assert!(fixture.remote_path_exists(&original_run_directory));
    assert!(fixture.remote_path_exists("/srv/track-testing/state/track-projects.json"));
    assert!(harness.task_exists(&task.id).await);
    assert!(harness.dispatch_history_exists(&task.id).await);

    let reset_response = harness.reset_remote_agent_workspace().await;
    let reset_summary = &reset_response["summary"];
    assert!(
        reset_summary["workspaceEntriesRemoved"]
            .as_u64()
            .expect("reset summary should include workspaceEntriesRemoved")
            >= 1
    );
    assert_eq!(reset_summary["registryRemoved"], true);

    assert!(!fixture.remote_path_exists("/home/track/workspace/project-a"));
    assert!(!fixture.remote_path_exists(&original_worktree_path));
    assert!(!fixture.remote_path_exists(&original_run_directory));
    assert!(!fixture.remote_path_exists("/srv/track-testing/state/track-projects.json"));
    assert!(harness.task_exists(&task.id).await);
    assert!(harness.dispatch_history_exists(&task.id).await);

    fixture.write_codex_state(&success_codex_state(
        0,
        Some("https://github.com/acme/project-a/pull/42"),
        false,
        "Mock Codex completed the post-reset dispatch successfully.",
    ));

    let second_dispatch = harness.dispatch_task(&task.id).await;
    let second_dispatch_id = second_dispatch["dispatchId"]
        .as_str()
        .expect("second dispatch response should include a dispatch id")
        .to_owned();
    let second_terminal_dispatch = harness
        .poll_dispatch_until_terminal(&task.id, Duration::from_secs(20))
        .await;
    assert_eq!(second_terminal_dispatch["status"], "succeeded");
    let second_worktree_path = second_terminal_dispatch["worktreePath"]
        .as_str()
        .expect("second terminal dispatch should include a worktree path")
        .to_owned();
    let second_run_directory =
        format!("/home/track/workspace/project-a/dispatches/{second_dispatch_id}");

    assert!(fixture.remote_path_exists("/home/track/workspace/project-a"));
    assert!(fixture.remote_path_exists(&second_worktree_path));
    assert!(fixture.remote_path_exists(&second_run_directory));
    assert!(fixture.remote_path_exists("/srv/track-testing/state/track-projects.json"));
}

#[tokio::test(flavor = "multi_thread")]
async fn resetting_remote_workspace_refuses_while_a_dispatch_is_active() {
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
        5,
        Some("https://github.com/acme/project-a/pull/42"),
        false,
        "Mock Codex is still running while reset is attempted.",
    ));

    let harness = ApiHarness::new(&fixture).await;
    let task = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Refuse remote reset while a dispatch is still running",
        )
        .await;

    let _first_dispatch = harness.dispatch_task(&task.id).await;

    let reset_error = harness
        .reset_remote_agent_workspace_expect_error(StatusCode::BAD_GATEWAY)
        .await;
    assert!(reset_error["error"]["message"]
        .as_str()
        .expect("error response should include a message")
        .contains(
            "Stop active remote task runs and PR reviews before resetting the remote workspace"
        ));
}

// =============================================================================
// Parallel Dispatch Tracking
// =============================================================================
//
// This case is intentionally not a load test. We first warm each project's
// checkout sequentially so the live assertion stays focused on the behavior we
// care about here: once remote bootstrap is in place, can the API track
// several concurrent dispatches across projects and return the right result for
// each task?
#[tokio::test(flavor = "multi_thread")]
async fn dispatches_three_tasks_in_parallel_across_two_projects() {
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
    fixture.seed_repo(
        "https://github.com/acme/project-b",
        "main",
        "fixture-user",
        "fixture-user",
    );

    let harness = ApiHarness::new(&fixture).await;

    fixture.write_codex_state(&success_codex_state(
        0,
        None,
        false,
        "Warm-up Codex run completed successfully.",
    ));
    let warm_project_a = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Warm the remote checkout for project-a",
        )
        .await;
    let warm_project_b = harness
        .create_task_with_project(
            "project-b",
            project_metadata("project-b"),
            "Warm the remote checkout for project-b",
        )
        .await;
    let _ = harness.dispatch_task(&warm_project_a.id).await;
    let _ = harness
        .poll_dispatch_until_terminal(&warm_project_a.id, Duration::from_secs(20))
        .await;
    let _ = harness.dispatch_task(&warm_project_b.id).await;
    let _ = harness
        .poll_dispatch_until_terminal(&warm_project_b.id, Duration::from_secs(20))
        .await;

    let codex_invocation_count_before_parallel = fixture.read_log_entries("codex").len();
    fixture.write_codex_state(&success_codex_state(
        2,
        None,
        false,
        "Parallel mock Codex run completed successfully.",
    ));

    let project_a_task_one = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Handle the first parallel task for project-a",
        )
        .await;
    let project_a_task_two = harness
        .create_task_with_project(
            "project-a",
            project_metadata("project-a"),
            "Handle the second parallel task for project-a",
        )
        .await;
    let project_b_task = harness
        .create_task_with_project(
            "project-b",
            project_metadata("project-b"),
            "Handle the parallel task for project-b",
        )
        .await;

    let queued_project_a_task_one = harness.dispatch_task(&project_a_task_one.id).await;
    let queued_project_a_task_two = harness.dispatch_task(&project_a_task_two.id).await;
    let queued_project_b_task = harness.dispatch_task(&project_b_task.id).await;
    assert_eq!(queued_project_a_task_one["status"], "preparing");
    assert_eq!(queued_project_a_task_two["status"], "preparing");
    assert_eq!(queued_project_b_task["status"], "preparing");

    let task_ids = vec![
        project_a_task_one.id.clone(),
        project_a_task_two.id.clone(),
        project_b_task.id.clone(),
    ];
    let terminal_dispatches = harness
        .poll_dispatches_until_all_terminal(&task_ids, Duration::from_secs(30))
        .await;

    assert_eq!(terminal_dispatches.len(), 3);

    let project_a_dispatch_one = terminal_dispatches
        .get(&project_a_task_one.id)
        .expect("project-a task one should have a dispatch");
    let project_a_dispatch_two = terminal_dispatches
        .get(&project_a_task_two.id)
        .expect("project-a task two should have a dispatch");
    let project_b_dispatch = terminal_dispatches
        .get(&project_b_task.id)
        .expect("project-b task should have a dispatch");

    assert_eq!(project_a_dispatch_one["status"], "succeeded");
    assert_eq!(project_a_dispatch_two["status"], "succeeded");
    assert_eq!(project_b_dispatch["status"], "succeeded");

    assert_ne!(
        project_a_dispatch_one["dispatchId"],
        project_a_dispatch_two["dispatchId"]
    );
    assert_ne!(
        project_a_dispatch_one["branchName"],
        project_a_dispatch_two["branchName"]
    );
    assert_ne!(
        project_a_dispatch_one["worktreePath"],
        project_a_dispatch_two["worktreePath"]
    );

    assert!(project_a_dispatch_one["worktreePath"]
        .as_str()
        .expect("project-a task one should have a worktree path")
        .contains("/project-a/worktrees/"));
    assert!(project_a_dispatch_two["worktreePath"]
        .as_str()
        .expect("project-a task two should have a worktree path")
        .contains("/project-a/worktrees/"));
    assert!(project_b_dispatch["worktreePath"]
        .as_str()
        .expect("project-b task should have a worktree path")
        .contains("/project-b/worktrees/"));

    let registry_contents =
        fixture.read_remote_file("/srv/track-testing/state/track-projects.json");
    assert!(registry_contents.contains("\"project-a\""));
    assert!(registry_contents.contains("\"project-b\""));

    assert_remote_dispatch_artifacts(
        &fixture,
        &project_a_task_one.project,
        project_a_dispatch_one,
    );
    assert_remote_dispatch_artifacts(
        &fixture,
        &project_a_task_two.project,
        project_a_dispatch_two,
    );
    assert_remote_dispatch_artifacts(&fixture, &project_b_task.project, project_b_dispatch);

    let parallel_codex_logs = fixture
        .read_log_entries("codex")
        .into_iter()
        .skip(codex_invocation_count_before_parallel)
        .collect::<Vec<_>>();
    assert_eq!(parallel_codex_logs.len(), 3);

    let logged_worktree_paths = parallel_codex_logs
        .iter()
        .map(|entry| {
            entry["result"]["worktreePath"]
                .as_str()
                .expect("codex log should include the worktree path")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    let expected_worktree_paths = [
        project_a_dispatch_one["worktreePath"]
            .as_str()
            .expect("project-a task one should have a worktree path")
            .to_owned(),
        project_a_dispatch_two["worktreePath"]
            .as_str()
            .expect("project-a task two should have a worktree path")
            .to_owned(),
        project_b_dispatch["worktreePath"]
            .as_str()
            .expect("project-b task should have a worktree path")
            .to_owned(),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(logged_worktree_paths, expected_worktree_paths);
}

fn project_metadata(project_name: &str) -> ProjectMetadata {
    ProjectMetadata {
        repo_url: format!("https://github.com/acme/{project_name}"),
        git_url: format!("/srv/track-testing/git/upstream/{project_name}.git"),
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

fn success_claude_state(
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
                "message": "fix: apply mocked claude change",
                "files": [
                    {
                        "path": "MOCK_CLAUDE_CHANGE.md",
                        "contents": "# Mock change\n\nThis file was created by the Claude fixture.\n"
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

fn review_claude_state(sleep_seconds: u64, summary: &str, review_body: &str) -> Value {
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

fn assert_remote_dispatch_artifacts(fixture: &RemoteFixture, project_name: &str, dispatch: &Value) {
    let dispatch_id = dispatch["dispatchId"]
        .as_str()
        .expect("dispatch should include a dispatch id");
    let remote_run_directory =
        format!("/home/track/workspace/{project_name}/dispatches/{dispatch_id}");

    let remote_status = fixture.read_remote_file(&format!("{remote_run_directory}/status.txt"));
    assert_eq!(remote_status.trim(), "completed");

    let remote_result = fixture.read_remote_file(&format!("{remote_run_directory}/result.json"));
    assert!(remote_result.contains("\"status\": \"succeeded\""));
}
