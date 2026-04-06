use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde_json::json;
use tempfile::TempDir;
use time::Duration;
use track_config::config::{
    ApiConfigFile, LlamaCppConfigFile, RemoteAgentConfigFile, TrackConfigFile,
};
use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::database::DatabaseContext;
use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::ids::{DispatchId, ProjectId, ReviewId, TaskId};
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, WorkspaceKey};
use track_types::test_support::{set_env_var, track_data_env_lock, EnvVarGuard};
use track_types::time_utils::now_utc;
use track_types::types::{
    DispatchStatus, Priority, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord, Status,
    Task, TaskCreateInput, TaskDispatchRecord, TaskSource, TaskUpdateInput,
};
use track_types::urls::Url;

use crate::types::RemoteDispatchSnapshot;

use super::dispatch::{
    latest_pull_request_for_branch, refresh_dispatch_record_from_snapshot,
    select_follow_up_base_dispatch, unique_run_directories,
};
use super::review::select_previous_submitted_review_run;
use super::{
    RemoteAgentConfigProvider, RemoteAgentServices, RemoteDispatchService, RemoteReviewService,
};

#[derive(Debug, Clone)]
pub(crate) struct StaticRemoteAgentConfigService {
    remote_agent: Option<RemoteAgentRuntimeConfig>,
}

impl StaticRemoteAgentConfigService {
    pub(crate) fn new(remote_agent: Option<RemoteAgentRuntimeConfig>) -> Self {
        Self { remote_agent }
    }
}

#[async_trait::async_trait]
impl RemoteAgentConfigProvider for StaticRemoteAgentConfigService {
    async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        Ok(self.remote_agent.clone())
    }
}

struct TestContext {
    _directory: TempDir,
    _env_lock_guard: std::sync::MutexGuard<'static, ()>,
    _track_state_dir_guard: EnvVarGuard,
    data_dir: PathBuf,
    config_service: StaticRemoteAgentConfigService,
    database: DatabaseContext,
}

impl TestContext {
    async fn new(config: TrackConfigFile) -> Self {
        let directory = TempDir::new().expect("tempdir should be created");
        let env_lock_guard = track_data_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state_root = directory.path().join("state");
        let track_state_dir_guard = set_env_var("TRACK_STATE_DIR", &state_root);
        let data_dir = state_root.join("issues");
        let database_path = state_root.join("track.sqlite");
        let config_service =
            StaticRemoteAgentConfigService::new(config.remote_agent.map(|remote_agent| {
                RemoteAgentRuntimeConfig {
                    host: remote_agent.host,
                    user: remote_agent.user,
                    port: remote_agent.port,
                    workspace_root: remote_agent.workspace_root,
                    projects_registry_path: remote_agent.projects_registry_path,
                    preferred_tool: remote_agent.preferred_tool,
                    shell_prelude: remote_agent.shell_prelude,
                    review_follow_up: remote_agent.review_follow_up.and_then(|review_follow_up| {
                        review_follow_up.main_user.map(|main_user| {
                            RemoteAgentReviewFollowUpRuntimeConfig {
                                enabled: review_follow_up.enabled,
                                main_user,
                                default_review_prompt: review_follow_up.default_review_prompt,
                            }
                        })
                    }),
                    managed_key_path: state_root.join("remote-agent").join("id_ed25519"),
                    managed_known_hosts_path: state_root.join("remote-agent").join("known_hosts"),
                }
            }));

        Self {
            _directory: directory,
            _env_lock_guard: env_lock_guard,
            _track_state_dir_guard: track_state_dir_guard,
            data_dir: data_dir.clone(),
            config_service,
            database: DatabaseContext::initialized(Some(database_path))
                .await
                .expect("database should resolve"),
        }
    }

    fn remote_agent_services(&self) -> RemoteAgentServices<'_> {
        RemoteAgentServices::new(&self.config_service, &self.database)
    }

    fn service(&self) -> RemoteDispatchService<'_> {
        self.remote_agent_services().dispatch()
    }

    fn review_service(&self) -> RemoteReviewService<'_> {
        self.remote_agent_services().review()
    }

    async fn create_task(&self, project: &str, description: &str) -> Task {
        let project_id = ProjectId::new(project).unwrap();
        self.database
            .project_repository()
            .upsert_project_by_name(
                &project_id,
                ProjectMetadata {
                    repo_url: url(&format!("https://github.com/acme/{project}")),
                    git_url: GitRemote::new(&format!("git@github.com:acme/{project}.git")).unwrap(),
                    base_branch: "main".to_owned(),
                    description: None,
                },
                Vec::new(),
            )
            .await
            .expect("project should save");
        self.database
            .task_repository()
            .create_task(TaskCreateInput {
                project: project_id,
                priority: Priority::High,
                description: description.to_owned(),
                source: Some(TaskSource::Web),
            })
            .await
            .expect("task should be created")
            .task
    }

    async fn write_project_metadata(&self, project: &str) {
        let project_id = ProjectId::new(project).unwrap();
        self.database
            .project_repository()
            .upsert_project_by_name(
                &project_id,
                ProjectMetadata {
                    repo_url: url(&format!("https://github.com/acme/{project}")),
                    git_url: GitRemote::new(&format!("git@github.com:acme/{project}.git")).unwrap(),
                    base_branch: "main".to_owned(),
                    description: None,
                },
                Vec::new(),
            )
            .await
            .expect("project metadata should save");
    }

    async fn create_running_dispatch(&self, task: &Task) -> TaskDispatchRecord {
        let dispatch_id = DispatchId::new(format!("dispatch-{}-running", task.id)).unwrap();
        let branch_name = DispatchBranch::for_task(&dispatch_id);
        let worktree_path =
            DispatchWorktreePath::for_task("~/workspace", &task.project, &dispatch_id);
        let mut dispatch = self
            .database
            .dispatch_repository()
            .create_dispatch(
                task,
                &dispatch_id,
                "198.51.100.10",
                RemoteAgentPreferredTool::Codex,
                &branch_name,
                &worktree_path,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("dispatch should be created");
        dispatch.status = DispatchStatus::Running;
        dispatch.summary =
            Some("The remote agent is working in the prepared environment.".to_owned());
        dispatch.updated_at = now_utc();
        self.database
            .dispatch_repository()
            .save_dispatch(&dispatch)
            .await
            .expect("dispatch should save");
        dispatch
    }

    async fn create_review(&self) -> ReviewRecord {
        let review = sample_review_record();
        self.database
            .review_repository()
            .save_review(&review)
            .await
            .expect("review should save");
        review
    }
}

fn base_test_config(remote_agent: Option<RemoteAgentConfigFile>) -> TrackConfigFile {
    TrackConfigFile {
        project_roots: Vec::new(),
        project_aliases: BTreeMap::new(),
        api: ApiConfigFile { port: 3210 },
        llama_cpp: LlamaCppConfigFile {
            model_path: Some("/tmp/parser.gguf".to_owned()),
            model_hf_repo: None,
            model_hf_file: None,
        },
        remote_agent,
    }
}

fn install_dummy_managed_remote_agent_material(data_dir: &Path) {
    let remote_agent_dir = data_dir
        .parent()
        .expect("data dir should have a parent")
        .join("remote-agent");
    fs::create_dir_all(&remote_agent_dir).expect("remote-agent dir should be created");
    fs::write(
        remote_agent_dir.join("id_ed25519"),
        "not-a-real-private-key",
    )
    .expect("dummy SSH key should be written");
    fs::write(remote_agent_dir.join("known_hosts"), "")
        .expect("dummy known_hosts file should be written");
}

fn url(value: &str) -> Url {
    Url::parse(value).unwrap()
}

fn sample_review_record() -> ReviewRecord {
    let created_at = now_utc();
    ReviewRecord {
        id: ReviewId::new("20260326-120000-review-pr-42").unwrap(),
        pull_request_url: url("https://github.com/acme/project-x/pull/42"),
        pull_request_number: 42,
        pull_request_title: "Fix queue layout".to_owned(),
        repository_full_name: "acme/project-x".to_owned(),
        repo_url: url("https://github.com/acme/project-x"),
        git_url: GitRemote::new("git@github.com:acme/project-x.git").unwrap(),
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

#[tokio::test]
async fn saved_review_dispatch_prerequisites_do_not_depend_on_live_review_follow_up_settings() {
    let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
        host: "127.0.0.1".to_owned(),
        user: "builder".to_owned(),
        port: 2222,
        workspace_root: "~/workspace".to_owned(),
        projects_registry_path: "~/track-projects.json".to_owned(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
        review_follow_up: None,
    })))
    .await;
    let review = context.create_review().await;

    let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
    install_dummy_managed_remote_agent_material(&context.data_dir);

    let (remote_agent, loaded_review) = context
        .review_service()
        .load_review_dispatch_prerequisites(&review.id)
        .await
        .expect("saved review dispatch prerequisites should load");

    assert_eq!(remote_agent.host, "127.0.0.1");
    assert_eq!(loaded_review.id, review.id);
    assert_eq!(loaded_review.main_user, review.main_user);
    assert_eq!(
        loaded_review.default_review_prompt,
        review.default_review_prompt
    );
}

#[test]
fn refresh_reads_claude_dispatch_outcome_from_structured_output_envelope() {
    let created_at = now_utc();
    let dispatch_id = DispatchId::new("dispatch-1").unwrap();
    let project = ProjectId::new("project-a").unwrap();
    let record = TaskDispatchRecord {
        dispatch_id: dispatch_id.clone(),
        task_id: TaskId::new("task-1").unwrap(),
        preferred_tool: RemoteAgentPreferredTool::Claude,
        project: project.clone(),
        status: DispatchStatus::Running,
        created_at,
        updated_at: created_at,
        finished_at: None,
        remote_host: "192.0.2.25".to_owned(),
        branch_name: Some(DispatchBranch::for_task(&dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_task(
            "~/workspace",
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
    };
    let snapshot = RemoteDispatchSnapshot::completed(
        json!({
            "result": "Mock Claude completed successfully.",
            "structured_output": {
                "status": "succeeded",
                "summary": "Mock Claude completed successfully.",
                "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
                "branchName": "track/dispatch-1",
                "worktreePath": "/tmp/project-a/worktrees/dispatch-1",
                "notes": "Captured from the Claude mock."
            }
        })
        .to_string(),
        "2026-03-18T10:35:31Z\n",
    );

    let refreshed = refresh_dispatch_record_from_snapshot(record, &snapshot)
        .expect("Claude envelope should refresh successfully");

    assert_eq!(refreshed.status, DispatchStatus::Succeeded);
    assert_eq!(
        refreshed.summary.as_deref(),
        Some("Mock Claude completed successfully.")
    );
    assert_eq!(
        refreshed.pull_request_url.as_ref().map(Url::as_str),
        Some("https://github.com/acme/project-a/pull/42")
    );
    assert_eq!(
        refreshed.worktree_path,
        Some(DispatchWorktreePath::new("/tmp/project-a/worktrees/dispatch-1").unwrap())
    );
    assert_eq!(
        refreshed.notes.as_deref(),
        Some("Captured from the Claude mock.")
    );
}

#[tokio::test]
async fn refresh_reads_claude_review_outcome_from_structured_output_envelope() {
    let context = TestContext::new(base_test_config(None)).await;
    let created_at = now_utc();
    let dispatch_id = DispatchId::new("review-dispatch-1").unwrap();
    let workspace_key = WorkspaceKey::new("project-a").unwrap();
    let record = ReviewRunRecord {
        dispatch_id: dispatch_id.clone(),
        review_id: ReviewId::new("review-1").unwrap(),
        pull_request_url: url("https://github.com/acme/project-a/pull/42"),
        repository_full_name: "acme/project-a".to_owned(),
        workspace_key: workspace_key.clone(),
        preferred_tool: RemoteAgentPreferredTool::Claude,
        status: DispatchStatus::Running,
        created_at,
        updated_at: created_at,
        finished_at: None,
        remote_host: "192.0.2.25".to_owned(),
        branch_name: Some(DispatchBranch::for_review(&dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_review(
            "~/workspace",
            &workspace_key,
            &dispatch_id,
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
    let snapshot = RemoteDispatchSnapshot::completed(
        json!({
            "result": "Mock Claude reviewed the pull request successfully.",
            "structured_output": {
                "status": "succeeded",
                "summary": "Mock Claude reviewed the pull request successfully.",
                "reviewSubmitted": true,
                "githubReviewId": "1001",
                "githubReviewUrl": "https://github.com/acme/project-a/pull/42#pullrequestreview-1001",
                "worktreePath": "/tmp/project-a/review-worktrees/review-dispatch-1",
                "notes": "Captured from the Claude review mock."
            }
        })
        .to_string(),
        "2026-03-18T10:35:31Z\n",
    );

    let refreshed = context
        .review_service()
        .refresh_review_dispatch_record_from_snapshot(record, &snapshot)
        .expect("Claude review envelope should refresh successfully");

    assert_eq!(refreshed.status, DispatchStatus::Succeeded);
    assert_eq!(
        refreshed.summary.as_deref(),
        Some("Mock Claude reviewed the pull request successfully.")
    );
    assert!(refreshed.review_submitted);
    assert_eq!(refreshed.github_review_id.as_deref(), Some("1001"));
    assert_eq!(
        refreshed.github_review_url.as_ref().map(Url::as_str),
        Some("https://github.com/acme/project-a/pull/42#pullrequestreview-1001")
    );
    assert_eq!(
        refreshed.worktree_path,
        Some(
            DispatchWorktreePath::new("/tmp/project-a/review-worktrees/review-dispatch-1").unwrap()
        )
    );
    assert_eq!(
        refreshed.notes.as_deref(),
        Some("Captured from the Claude review mock.")
    );
}

#[test]
fn refresh_marks_remote_canceled_runs_as_terminal() {
    let created_at = now_utc();
    let dispatch_id = DispatchId::new("dispatch-1").unwrap();
    let project = ProjectId::new("project-a").unwrap();
    let record = TaskDispatchRecord {
        dispatch_id: dispatch_id.clone(),
        task_id: TaskId::new("task-1").unwrap(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        project: project.clone(),
        status: DispatchStatus::Running,
        created_at,
        updated_at: created_at,
        finished_at: None,
        remote_host: "192.0.2.25".to_owned(),
        branch_name: Some(DispatchBranch::for_task(&dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_task(
            "~/workspace",
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
    };
    let snapshot = RemoteDispatchSnapshot::canceled("2026-03-18T10:35:31Z\n");

    let refreshed = refresh_dispatch_record_from_snapshot(record, &snapshot)
        .expect("canceled snapshot should refresh");

    assert_eq!(refreshed.status, DispatchStatus::Canceled);
    assert_eq!(
        refreshed.summary.as_deref(),
        Some("Canceled from the web UI.")
    );
    assert!(refreshed.finished_at.is_some());
}

#[test]
fn follow_up_uses_the_latest_reusable_dispatch_context() {
    let created_at = now_utc();
    let dispatch_history = vec![
        TaskDispatchRecord {
            dispatch_id: DispatchId::new("dispatch-3").unwrap(),
            task_id: TaskId::new("task-1").unwrap(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: ProjectId::new("project-a").unwrap(),
            status: DispatchStatus::Failed,
            created_at: created_at + Duration::seconds(2),
            updated_at: created_at + Duration::seconds(2),
            finished_at: Some(created_at + Duration::seconds(2)),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: None,
            worktree_path: None,
            pull_request_url: None,
            follow_up_request: Some("Address review comments".to_owned()),
            summary: Some("Launch failed before the branch was restored.".to_owned()),
            notes: None,
            error_message: Some("Remote launch failed.".to_owned()),
            review_request_head_oid: None,
            review_request_user: None,
        },
        TaskDispatchRecord {
            dispatch_id: DispatchId::new("dispatch-2").unwrap(),
            task_id: TaskId::new("task-1").unwrap(),
            preferred_tool: RemoteAgentPreferredTool::Claude,
            project: ProjectId::new("project-a").unwrap(),
            status: DispatchStatus::Succeeded,
            created_at: created_at + Duration::seconds(1),
            updated_at: created_at + Duration::seconds(1),
            finished_at: Some(created_at + Duration::seconds(1)),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some(DispatchBranch::for_task(
                &DispatchId::new("dispatch-2").unwrap(),
            )),
            worktree_path: Some(DispatchWorktreePath::for_task(
                "~/workspace",
                &ProjectId::new("project-a").unwrap(),
                &DispatchId::new("dispatch-2").unwrap(),
            )),
            pull_request_url: Some(url("https://github.com/acme/project-a/pull/42")),
            follow_up_request: None,
            summary: Some("Opened a PR.".to_owned()),
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        },
        TaskDispatchRecord {
            dispatch_id: DispatchId::new("dispatch-1").unwrap(),
            task_id: TaskId::new("task-1").unwrap(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            project: ProjectId::new("project-a").unwrap(),
            status: DispatchStatus::Failed,
            created_at,
            updated_at: created_at,
            finished_at: Some(created_at),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some(DispatchBranch::for_task(
                &DispatchId::new("dispatch-1").unwrap(),
            )),
            worktree_path: Some(DispatchWorktreePath::for_task(
                "~/workspace",
                &ProjectId::new("project-a").unwrap(),
                &DispatchId::new("dispatch-1").unwrap(),
            )),
            pull_request_url: Some(url("https://github.com/acme/project-a/pull/1")),
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: Some("Old failure.".to_owned()),
            review_request_head_oid: None,
            review_request_user: None,
        },
    ];

    let selected = select_follow_up_base_dispatch(&TaskId::new("fake").unwrap(), &dispatch_history)
        .expect("a reusable dispatch should be selected");
    let pull_request_url = latest_pull_request_for_branch(
        &dispatch_history,
        selected
            .branch_name
            .as_ref()
            .expect("selected dispatch should have a branch name"),
    );

    assert_eq!(selected.dispatch_id, "dispatch-2");
    assert_eq!(
        pull_request_url.as_ref().map(Url::as_str),
        Some("https://github.com/acme/project-a/pull/42")
    );
}

#[test]
fn follow_up_dispatch_run_directories_use_the_current_dispatch_id() {
    let remote_agent = RemoteAgentRuntimeConfig {
        host: "127.0.0.1".to_owned(),
        user: "builder".to_owned(),
        port: 2222,
        workspace_root: "/home/track/workspace".to_owned(),
        projects_registry_path: "/home/track/track-projects.json".to_owned(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        shell_prelude: None,
        review_follow_up: None,
        managed_key_path: PathBuf::from("/tmp/id_ed25519"),
        managed_known_hosts_path: PathBuf::from("/tmp/known_hosts"),
    };
    let original_dispatch_id = DispatchId::new("dispatch-1").unwrap();
    let follow_up_dispatch_id = DispatchId::new("dispatch-2").unwrap();
    let project = ProjectId::new("project-a").unwrap();
    let follow_up_record = TaskDispatchRecord {
        dispatch_id: follow_up_dispatch_id,
        task_id: TaskId::new("task-1").unwrap(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        project: project.clone(),
        status: DispatchStatus::Running,
        created_at: now_utc(),
        updated_at: now_utc(),
        finished_at: None,
        remote_host: "192.0.2.25".to_owned(),
        branch_name: Some(DispatchBranch::for_task(&original_dispatch_id)),
        worktree_path: Some(DispatchWorktreePath::for_task(
            "/home/track/workspace",
            &project,
            &original_dispatch_id,
        )),
        pull_request_url: Some(url("https://github.com/acme/project-a/pull/42")),
        follow_up_request: Some("Continue the existing PR.".to_owned()),
        summary: None,
        notes: None,
        error_message: None,
        review_request_head_oid: None,
        review_request_user: None,
    };

    let run_directories = unique_run_directories(&[follow_up_record], &remote_agent);

    assert_eq!(
        run_directories,
        vec!["/home/track/workspace/project-a/dispatches/dispatch-2".to_owned()]
    );
}

#[test]
fn selects_the_latest_previous_submitted_review_run() {
    let review = sample_review_record();
    let dispatch_history = vec![
        ReviewRunRecord {
            dispatch_id: DispatchId::new("dispatch-3").unwrap(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Preparing,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: None,
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some(DispatchBranch::for_review(
                &DispatchId::new("dispatch-3").unwrap(),
            )),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &review.workspace_key,
                &DispatchId::new("dispatch-3").unwrap(),
            )),
            follow_up_request: Some("Re-review the latest fixes.".to_owned()),
            target_head_oid: Some("ccc333".to_owned()),
            summary: Some("Re-review request: Re-review the latest fixes.".to_owned()),
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        },
        ReviewRunRecord {
            dispatch_id: DispatchId::new("dispatch-2").unwrap(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Succeeded,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: Some(now_utc()),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some(DispatchBranch::for_review(
                &DispatchId::new("dispatch-2").unwrap(),
            )),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &review.workspace_key,
                &DispatchId::new("dispatch-2").unwrap(),
            )),
            follow_up_request: None,
            target_head_oid: Some("bbb222".to_owned()),
            summary: Some("Submitted a review.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1002".to_owned()),
            github_review_url: Some(url(
                "https://github.com/acme/project-x/pull/42#pullrequestreview-1002",
            )),
            notes: None,
            error_message: None,
        },
        ReviewRunRecord {
            dispatch_id: DispatchId::new("dispatch-1").unwrap(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: review.preferred_tool,
            status: DispatchStatus::Succeeded,
            created_at: now_utc(),
            updated_at: now_utc(),
            finished_at: Some(now_utc()),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some(DispatchBranch::for_review(
                &DispatchId::new("dispatch-1").unwrap(),
            )),
            worktree_path: Some(DispatchWorktreePath::for_review(
                "~/workspace",
                &review.workspace_key,
                &DispatchId::new("dispatch-1").unwrap(),
            )),
            follow_up_request: None,
            target_head_oid: Some("aaa111".to_owned()),
            summary: Some("Submitted an older review.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(url(
                "https://github.com/acme/project-x/pull/42#pullrequestreview-1001",
            )),
            notes: None,
            error_message: None,
        },
    ];

    let selected = select_previous_submitted_review_run(
        &dispatch_history,
        &DispatchId::new("dispatch-3").unwrap(),
    )
    .expect("a previous submitted review should be selected");

    assert_eq!(selected.dispatch_id, "dispatch-2");
    assert_eq!(selected.github_review_id.as_deref(), Some("1002"));
}

#[tokio::test]
async fn closing_task_stays_local_when_remote_cleanup_is_unavailable() {
    let context = TestContext::new(base_test_config(None)).await;
    let task = context
        .create_task("project-a", "Investigate a flaky remote cleanup")
        .await;
    let existing_dispatch = context.create_running_dispatch(&task).await;

    let updated_task = context
        .service()
        .update_task(
            &task.id,
            TaskUpdateInput {
                status: Some(Status::Closed),
                ..TaskUpdateInput::default()
            },
        )
        .await
        .expect("closing should still succeed locally");

    let updated_dispatch = context
        .database
        .dispatch_repository()
        .get_dispatch(&task.id, &existing_dispatch.dispatch_id)
        .await
        .expect("dispatch lookup should succeed")
        .expect("dispatch should still exist");

    assert_eq!(updated_task.status, Status::Closed);
    assert_eq!(updated_dispatch.status, DispatchStatus::Canceled);
    assert_eq!(
        updated_dispatch.summary.as_deref(),
        Some("Canceled because the task was closed locally. Remote cleanup was skipped.")
    );
    assert!(updated_dispatch
        .error_message
        .as_deref()
        .is_some_and(|message| message.contains("Remote agent configuration is missing")));
}

#[tokio::test]
async fn deleting_task_stays_local_when_remote_cleanup_is_unavailable() {
    let context = TestContext::new(base_test_config(None)).await;
    let task = context
        .create_task("project-a", "Delete the task even without remote cleanup")
        .await;
    let _existing_dispatch = context.create_running_dispatch(&task).await;

    context
        .service()
        .delete_task(&task.id)
        .await
        .expect("delete should still succeed locally");

    let task_error = context
        .database
        .task_repository()
        .get_task(&task.id)
        .await
        .expect_err("deleted task should be gone");
    assert_eq!(task_error.code, ErrorCode::TaskNotFound);
    assert!(context
        .database
        .dispatch_repository()
        .dispatches_for_task(&task.id)
        .await
        .expect("dispatch lookup should succeed")
        .is_empty());
}

#[tokio::test]
async fn refresh_releases_active_dispatches_when_remote_config_disappears() {
    let context = TestContext::new(base_test_config(None)).await;
    let task = context
        .create_task("project-a", "Recover from a missing remote config")
        .await;
    let existing_dispatch = context.create_running_dispatch(&task).await;

    let refreshed = context
        .service()
        .latest_dispatches_for_tasks(std::slice::from_ref(&task.id))
        .await
        .expect("dispatch refresh should succeed");
    let updated_dispatch = refreshed
        .first()
        .expect("latest dispatch should still be returned");

    assert_eq!(updated_dispatch.dispatch_id, existing_dispatch.dispatch_id);
    assert_eq!(updated_dispatch.status, DispatchStatus::Blocked);
    assert_eq!(
        updated_dispatch.summary.as_deref(),
        Some("Remote reconciliation is unavailable locally, so active runs were released.")
    );
    assert_eq!(
        updated_dispatch.error_message.as_deref(),
        Some("Remote agent configuration is missing locally.")
    );
}

#[tokio::test]
async fn queue_dispatch_releases_stale_active_dispatch_when_remote_refresh_fails() {
    let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
        host: "127.0.0.1".to_owned(),
        user: "builder".to_owned(),
        port: 1,
        workspace_root: "~/workspace".to_owned(),
        projects_registry_path: "~/track-projects.json".to_owned(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
        review_follow_up: None,
    })))
    .await;
    let task = context
        .create_task("project-a", "Retry after the previous remote run got stuck")
        .await;
    context.write_project_metadata(&task.project).await;
    let existing_dispatch = context.create_running_dispatch(&task).await;

    let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
    install_dummy_managed_remote_agent_material(&context.data_dir);

    let queued_dispatch = context
        .service()
        .queue_dispatch(&task.id, None)
        .await
        .expect("queueing should release the stale active dispatch first");
    let released_dispatch = context
        .database
        .dispatch_repository()
        .get_dispatch(&task.id, &existing_dispatch.dispatch_id)
        .await
        .expect("dispatch lookup should succeed")
        .expect("previous dispatch should still exist");

    assert_ne!(queued_dispatch.dispatch_id, existing_dispatch.dispatch_id);
    assert_eq!(queued_dispatch.status, DispatchStatus::Preparing);
    assert_eq!(released_dispatch.status, DispatchStatus::Blocked);
    assert_eq!(
            released_dispatch.summary.as_deref(),
            Some(
                "Remote reconciliation could not reach the remote machine, so active runs were released locally."
            )
        );
    assert!(released_dispatch.error_message.is_some());
}

#[tokio::test]
async fn follow_up_dispatch_keeps_the_original_runner_tool() {
    let context = TestContext::new(base_test_config(Some(RemoteAgentConfigFile {
        host: "127.0.0.1".to_owned(),
        user: "builder".to_owned(),
        port: 2222,
        workspace_root: "~/workspace".to_owned(),
        projects_registry_path: "~/track-projects.json".to_owned(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        shell_prelude: Some("export PATH=\"$PATH\"".to_owned()),
        review_follow_up: None,
    })))
    .await;
    let task = context
        .create_task("project-a", "Keep using the same runner on follow-up")
        .await;
    context.write_project_metadata(&task.project).await;
    let _track_data_dir = set_env_var("TRACK_DATA_DIR", &context.data_dir);
    install_dummy_managed_remote_agent_material(&context.data_dir);

    let mut first_dispatch = context
        .service()
        .queue_dispatch(&task.id, Some(RemoteAgentPreferredTool::Claude))
        .await
        .expect("initial dispatch should queue");
    first_dispatch.status = DispatchStatus::Succeeded;
    first_dispatch.finished_at = Some(first_dispatch.updated_at);
    context
        .database
        .dispatch_repository()
        .save_dispatch(&first_dispatch)
        .await
        .expect("initial dispatch should save as terminal");

    let follow_up_dispatch = context
        .service()
        .queue_follow_up_dispatch(&task.id, "Address the review comments.")
        .await
        .expect("follow-up dispatch should queue");

    assert_eq!(
        first_dispatch.preferred_tool,
        RemoteAgentPreferredTool::Claude
    );
    assert_eq!(
        follow_up_dispatch.preferred_tool,
        RemoteAgentPreferredTool::Claude
    );
}

#[test]
fn task_dispatch_start_guard_serializes_same_task() {
    let acquired_in_second_thread = Arc::new(AtomicBool::new(false));
    let guard = super::dispatch::TaskDispatchStartGuard::acquire("task-1");

    std::thread::scope(|scope| {
        let acquired_in_second_thread_for_join = Arc::clone(&acquired_in_second_thread);
        let join_handle = scope.spawn(move || {
            let _guard = super::dispatch::TaskDispatchStartGuard::acquire("task-1");
            acquired_in_second_thread_for_join.store(true, Ordering::SeqCst);
        });

        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !acquired_in_second_thread.load(Ordering::SeqCst),
            "the second same-task start should stay blocked while the first guard is held",
        );

        drop(guard);
        join_handle
            .join()
            .expect("second thread should acquire the guard after release");
    });

    assert!(
        acquired_in_second_thread.load(Ordering::SeqCst),
        "the waiting same-task start should proceed after the first guard releases",
    );
}

#[test]
fn review_dispatch_start_guard_serializes_same_review() {
    let acquired_in_second_thread = Arc::new(AtomicBool::new(false));
    let guard = super::review::ReviewDispatchStartGuard::acquire("review-1");

    std::thread::scope(|scope| {
        let acquired_in_second_thread_for_join = Arc::clone(&acquired_in_second_thread);
        let join_handle = scope.spawn(move || {
            let _guard = super::review::ReviewDispatchStartGuard::acquire("review-1");
            acquired_in_second_thread_for_join.store(true, Ordering::SeqCst);
        });

        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !acquired_in_second_thread.load(Ordering::SeqCst),
            "the second same-review start should stay blocked while the first guard is held",
        );

        drop(guard);
        join_handle
            .join()
            .expect("second thread should acquire the guard after release");
    });

    assert!(
        acquired_in_second_thread.load(Ordering::SeqCst),
        "the waiting same-review start should proceed after the first guard releases",
    );
}
