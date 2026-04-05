use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tempfile::TempDir;
use tower::ServiceExt;
use track_config::config::{
    RemoteAgentConfigFile, DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use track_config::paths::{
    get_backend_managed_remote_agent_key_path, get_backend_managed_remote_agent_known_hosts_path,
};
use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::settings_repository::SettingsRepository;
use track_dal::task_repository::FileTaskRepository;
use track_projects::project_catalog::ProjectInfo;
use track_projects::project_metadata::ProjectMetadata;
use track_types::time_utils::now_utc;
use track_types::types::{
    DispatchStatus, Priority, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord,
    TaskCreateInput, TaskSource,
};

use super::{build_app, AppState};
use crate::backend_config::{BackendConfigRepository, RemoteAgentConfigService};
use crate::migration_service::MigrationService;
use crate::test_support::{set_env_var, track_data_env_lock, EnvVarGuard};

fn static_root(directory: &TempDir) -> std::path::PathBuf {
    let root = directory.path().join("static");
    fs::create_dir_all(&root).expect("static root should exist");
    fs::write(root.join("index.html"), "<html><body>track</body></html>")
        .expect("static index should be written");
    root
}

fn database_path(directory: &TempDir) -> PathBuf {
    directory.path().join("backend").join("track.sqlite")
}

struct TestEnvironment {
    _env_lock: std::sync::MutexGuard<'static, ()>,
    _track_data_dir_guard: EnvVarGuard,
    _track_state_dir_guard: EnvVarGuard,
    _track_legacy_root_guard: EnvVarGuard,
    _track_legacy_config_guard: EnvVarGuard,
}

impl TestEnvironment {
    fn new(directory: &TempDir) -> Self {
        let env_lock = track_data_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let backend_state_dir = directory.path().join("backend");
        let backend_data_dir = backend_state_dir.join("issues");
        let legacy_root = directory.path().join("legacy-root");
        let legacy_config_path = directory.path().join("legacy-config/config.json");

        Self {
            _env_lock: env_lock,
            _track_data_dir_guard: set_env_var("TRACK_DATA_DIR", &backend_data_dir),
            _track_state_dir_guard: set_env_var("TRACK_STATE_DIR", &backend_state_dir),
            _track_legacy_root_guard: set_env_var("TRACK_LEGACY_ROOT", &legacy_root),
            _track_legacy_config_guard: set_env_var(
                "TRACK_LEGACY_CONFIG_PATH",
                &legacy_config_path,
            ),
        }
    }
}

async fn config_service(directory: &TempDir) -> Arc<RemoteAgentConfigService> {
    let database = DatabaseContext::new(Some(database_path(directory)))
        .await
        .expect("database context should resolve");
    let settings = SettingsRepository::new(Some(database))
        .await
        .expect("settings repository should resolve");
    let repository = BackendConfigRepository::new(Some(settings))
        .await
        .expect("backend config repository should resolve");

    Arc::new(
        RemoteAgentConfigService::new(Some(repository))
            .await
            .expect("remote-agent config service should resolve"),
    )
}

async fn dispatch_repository(directory: &TempDir) -> Arc<DispatchRepository> {
    Arc::new(
        DispatchRepository::new(Some(database_path(directory)))
            .await
            .expect("dispatch repository should resolve"),
    )
}

async fn project_repository(directory: &TempDir) -> Arc<ProjectRepository> {
    Arc::new(
        ProjectRepository::new(Some(database_path(directory)))
            .await
            .expect("project repository should resolve"),
    )
}

async fn review_repository(directory: &TempDir) -> Arc<ReviewRepository> {
    Arc::new(
        ReviewRepository::new(Some(database_path(directory)))
            .await
            .expect("review repository should resolve"),
    )
}

async fn review_dispatch_repository(directory: &TempDir) -> Arc<ReviewDispatchRepository> {
    Arc::new(
        ReviewDispatchRepository::new(Some(database_path(directory)))
            .await
            .expect("review dispatch repository should resolve"),
    )
}

async fn task_repository(directory: &TempDir) -> Arc<FileTaskRepository> {
    Arc::new(
        FileTaskRepository::new(Some(database_path(directory)))
            .await
            .expect("task repository should resolve"),
    )
}

fn migration_service(
    config_service: &Arc<RemoteAgentConfigService>,
    dispatch_repository: &Arc<DispatchRepository>,
    project_repository: &Arc<ProjectRepository>,
    review_dispatch_repository: &Arc<ReviewDispatchRepository>,
    review_repository: &Arc<ReviewRepository>,
    task_repository: &Arc<FileTaskRepository>,
) -> Arc<MigrationService> {
    Arc::new(
        MigrationService::new(
            (**config_service).clone(),
            (**project_repository).clone(),
            (**task_repository).clone(),
            (**dispatch_repository).clone(),
            (**review_repository).clone(),
            (**review_dispatch_repository).clone(),
        )
        .expect("migration service should resolve"),
    )
}

fn app_state(
    config_service: Arc<RemoteAgentConfigService>,
    dispatch_repository: Arc<DispatchRepository>,
    project_repository: Arc<ProjectRepository>,
    review_dispatch_repository: Arc<ReviewDispatchRepository>,
    review_repository: Arc<ReviewRepository>,
    task_repository: Arc<FileTaskRepository>,
) -> AppState {
    let migration_service = migration_service(
        &config_service,
        &dispatch_repository,
        &project_repository,
        &review_dispatch_repository,
        &review_repository,
        &task_repository,
    );

    AppState {
        config_service,
        dispatch_repository,
        migration_service,
        project_repository,
        review_dispatch_repository,
        review_repository,
        task_repository,
        task_change_version: Arc::new(AtomicU64::new(0)),
    }
}

async fn register_project(project_repository: &ProjectRepository, canonical_name: &str) {
    project_repository
        .upsert_project_by_name(
            canonical_name,
            ProjectMetadata {
                repo_url: format!("https://example.com/{canonical_name}"),
                git_url: format!("git@example.com:{canonical_name}.git"),
                base_branch: "main".to_owned(),
                description: None,
            },
            Vec::new(),
        )
        .await
        .expect("project should save");
}

async fn configured_remote_agent_config_service(
    directory: &TempDir,
) -> Arc<RemoteAgentConfigService> {
    let service = config_service(directory).await;
    service
        .save_remote_agent_config(Some(&RemoteAgentConfigFile {
            host: "192.0.2.25".to_owned(),
            user: "builder".to_owned(),
            port: DEFAULT_REMOTE_AGENT_PORT,
            workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
            projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some(". \"$HOME/.cargo/env\"".to_owned()),
            review_follow_up: None,
        }))
        .await
        .expect("remote-agent config should save");
    service
}

#[tokio::test]
async fn lists_tasks_with_backend_sorting() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let repository = task_repository(&directory).await;
    repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            description: "Middle priority task".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("first task should be created");
    repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::High,
            description: "Top priority task".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("second task should be created");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            repository,
        ),
        &static_root,
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/tasks")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response should be valid json");
    assert_eq!(json["tasks"][0]["priority"], "high");
}

#[tokio::test]
async fn creates_tasks_from_the_web_api() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let repository = task_repository(&directory).await;

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            repository.clone(),
        ),
        &static_root,
    );

    let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project":"project-a","priority":"high","description":"Create a task from the web UI"}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response should be valid json");
    assert_eq!(json["project"], "project-a");
    assert_eq!(json["priority"], "high");
    assert_eq!(json["source"], "web");

    let stored = repository
        .list_tasks(false, Some("project-a"))
        .await
        .expect("stored tasks should load");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].source, Some(TaskSource::Web));
}

#[tokio::test]
async fn preserves_cli_source_when_task_is_created_through_the_api() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let repository = task_repository(&directory).await;

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            repository.clone(),
        ),
        &static_root,
    );

    let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project":"project-a","priority":"high","description":"Create a task from the CLI","source":"cli"}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response should be valid json");
    assert_eq!(json["source"], "cli");

    let stored = repository
        .list_tasks(false, Some("project-a"))
        .await
        .expect("stored tasks should load");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].source, Some(TaskSource::Cli));
}

#[tokio::test]
async fn lists_dispatches_for_single_and_repeated_task_ids() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    register_project(&project_repository, "project-b").await;
    let task_repository = task_repository(&directory).await;
    let dispatch_repository = dispatch_repository(&directory).await;

    let first_task = task_repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::High,
            description: "First dispatched task".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("first task should be created")
        .task;
    let second_task = task_repository
        .create_task(TaskCreateInput {
            project: "project-b".to_owned(),
            priority: Priority::Medium,
            description: "Second dispatched task".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("second task should be created")
        .task;

    let mut first_dispatch = dispatch_repository
        .create_dispatch(
            &first_task,
            "dispatch-first",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-first",
            "/tmp/track/project-a/worktrees/dispatch-first",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("first dispatch should be created");
    first_dispatch.status = DispatchStatus::Succeeded;
    first_dispatch.finished_at = Some(first_dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&first_dispatch)
        .await
        .expect("first dispatch should save");

    let mut second_dispatch = dispatch_repository
        .create_dispatch(
            &second_task,
            "dispatch-second",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-second",
            "/tmp/track/project-b/worktrees/dispatch-second",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("second dispatch should be created");
    second_dispatch.status = DispatchStatus::Succeeded;
    second_dispatch.finished_at = Some(second_dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&second_dispatch)
        .await
        .expect("second dispatch should save");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository,
        ),
        &static_root,
    );

    let single_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/dispatches?taskId={}", first_task.id))
                .body(Body::empty())
                .expect("single-dispatch request should build"),
        )
        .await
        .expect("single-dispatch request should succeed");
    assert_eq!(single_response.status(), StatusCode::OK);
    let single_body = axum::body::to_bytes(single_response.into_body(), usize::MAX)
        .await
        .expect("single-dispatch body should be readable");
    let single_json: serde_json::Value =
        serde_json::from_slice(&single_body).expect("single-dispatch response should be json");
    assert_eq!(single_json["dispatches"].as_array().map(Vec::len), Some(1));
    assert_eq!(single_json["dispatches"][0]["taskId"], first_task.id);

    let repeated_response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/dispatches?taskId={}&taskId={}",
                    first_task.id, second_task.id
                ))
                .body(Body::empty())
                .expect("repeated-dispatch request should build"),
        )
        .await
        .expect("repeated-dispatch request should succeed");
    assert_eq!(repeated_response.status(), StatusCode::OK);
    let repeated_body = axum::body::to_bytes(repeated_response.into_body(), usize::MAX)
        .await
        .expect("repeated-dispatch body should be readable");
    let repeated_json: serde_json::Value =
        serde_json::from_slice(&repeated_body).expect("repeated-dispatch response should be json");
    assert_eq!(
        repeated_json["dispatches"].as_array().map(Vec::len),
        Some(2)
    );
}

#[tokio::test]
async fn lists_runs_with_task_context() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let task_repository = task_repository(&directory).await;
    let dispatch_repository = dispatch_repository(&directory).await;

    let task = task_repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::High,
            description: "Investigate an agent run".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("task should be created")
        .task;
    let mut dispatch = dispatch_repository
        .create_dispatch(
            &task,
            "dispatch-single",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-single",
            "/tmp/track/project-a/worktrees/dispatch-single",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("dispatch should be created");
    dispatch.status = DispatchStatus::Succeeded;
    dispatch.finished_at = Some(dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&dispatch)
        .await
        .expect("dispatch should save");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository,
        ),
        &static_root,
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/runs?limit=10")
                .body(Body::empty())
                .expect("runs request should build"),
        )
        .await
        .expect("runs request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("runs response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("runs response should be valid json");

    assert_eq!(json["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["runs"][0]["task"]["id"], task.id);
    assert_eq!(
        json["runs"][0]["dispatch"]["dispatchId"],
        dispatch.dispatch_id
    );
}

#[tokio::test]
async fn lists_task_scoped_runs_without_global_limit_truncation() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let task_repository = task_repository(&directory).await;
    let dispatch_repository = dispatch_repository(&directory).await;

    let task = task_repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::High,
            description: "Inspect task-scoped run history".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("task should be created")
        .task;

    let mut first_dispatch = dispatch_repository
        .create_dispatch(
            &task,
            "dispatch-history-first",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-history-first",
            "/tmp/track/project-a/worktrees/dispatch-history-first",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("first dispatch should be created");
    first_dispatch.status = DispatchStatus::Succeeded;
    first_dispatch.finished_at = Some(first_dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&first_dispatch)
        .await
        .expect("first dispatch should save");

    let mut second_dispatch = dispatch_repository
        .create_dispatch(
            &task,
            "dispatch-history-second",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-history-second",
            "/tmp/track/project-a/worktrees/dispatch-history-second",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("second dispatch should be created");
    second_dispatch.status = DispatchStatus::Succeeded;
    second_dispatch.finished_at = Some(second_dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&second_dispatch)
        .await
        .expect("second dispatch should save");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository,
        ),
        &static_root,
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/tasks/{}/runs", task.id))
                .body(Body::empty())
                .expect("task-runs request should build"),
        )
        .await
        .expect("task-runs request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("task-runs response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("task-runs response should be valid json");

    assert_eq!(json["runs"].as_array().map(Vec::len), Some(2));
    assert!(json["runs"]
        .as_array()
        .expect("runs should be an array")
        .iter()
        .all(|run| run["task"]["id"] == task.id));
}

#[tokio::test]
async fn lists_reviews_with_latest_run_and_review_history() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let review_repository = review_repository(&directory).await;
    let review_dispatch_repository = review_dispatch_repository(&directory).await;
    let created_at = now_utc();
    let review = ReviewRecord {
        id: "20260326-120000-review-pr-42".to_owned(),
        pull_request_url: "https://github.com/acme/project-a/pull/42".to_owned(),
        pull_request_number: 42,
        pull_request_title: "Fix queue layout".to_owned(),
        repository_full_name: "acme/project-a".to_owned(),
        repo_url: "https://github.com/acme/project-a".to_owned(),
        git_url: "git@github.com:acme/project-a.git".to_owned(),
        base_branch: "main".to_owned(),
        workspace_key: "project-a".to_owned(),
        preferred_tool: RemoteAgentPreferredTool::Codex,
        project: Some("project-a".to_owned()),
        main_user: "octocat".to_owned(),
        default_review_prompt: Some("Focus on regressions.".to_owned()),
        extra_instructions: Some("Pay attention to queue layout.".to_owned()),
        created_at,
        updated_at: created_at,
    };
    review_repository
        .save_review(&review)
        .await
        .expect("review should save");
    review_dispatch_repository
        .save_dispatch(&ReviewRunRecord {
            dispatch_id: "review-dispatch-1".to_owned(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            status: DispatchStatus::Succeeded,
            created_at,
            updated_at: created_at,
            finished_at: Some(created_at),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track-review/review-dispatch-1".to_owned()),
            worktree_path: Some("/tmp/review-worktree".to_owned()),
            follow_up_request: None,
            target_head_oid: Some("abc123def456".to_owned()),
            summary: Some("Submitted a GitHub review with two inline comments.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(
                "https://github.com/acme/project-a/pull/42#pullrequestreview-1001".to_owned(),
            ),
            notes: None,
            error_message: None,
        })
        .await
        .expect("review run should save");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository(&directory).await,
            review_dispatch_repository,
            review_repository,
            task_repository(&directory).await,
        ),
        &static_root,
    );

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/reviews")
                .body(Body::empty())
                .expect("review list request should build"),
        )
        .await
        .expect("review list request should succeed");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .expect("review list response body should be readable");
    let list_json: serde_json::Value =
        serde_json::from_slice(&list_body).expect("review list response should be valid json");
    assert_eq!(list_json["reviews"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        list_json["reviews"][0]["latestRun"]["reviewSubmitted"],
        true
    );

    let runs_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/reviews/{}/runs", review.id))
                .body(Body::empty())
                .expect("review runs request should build"),
        )
        .await
        .expect("review runs request should succeed");
    assert_eq!(runs_response.status(), StatusCode::OK);
    let runs_body = axum::body::to_bytes(runs_response.into_body(), usize::MAX)
        .await
        .expect("review runs response body should be readable");
    let runs_json: serde_json::Value =
        serde_json::from_slice(&runs_body).expect("review runs response should be valid json");
    assert_eq!(runs_json["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(runs_json["runs"][0]["reviewSubmitted"], true);
    assert_eq!(
        runs_json["runs"][0]["summary"],
        "Submitted a GitHub review with two inline comments."
    );
    assert_eq!(
        runs_json["runs"][0]["githubReviewUrl"],
        "https://github.com/acme/project-a/pull/42#pullrequestreview-1001"
    );
}

#[tokio::test]
async fn discards_dispatch_history_for_a_task() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let task_repository = task_repository(&directory).await;
    let dispatch_repository = dispatch_repository(&directory).await;

    let task = task_repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::High,
            description: "Discardable dispatch".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("task should be created")
        .task;

    let mut dispatch = dispatch_repository
        .create_dispatch(
            &task,
            "dispatch-terminal",
            "192.0.2.25",
            RemoteAgentPreferredTool::Codex,
            "track/dispatch-terminal",
            "/tmp/track/project-a/worktrees/dispatch-terminal",
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("dispatch should be created");
    dispatch.status = DispatchStatus::Failed;
    dispatch.finished_at = Some(dispatch.updated_at);
    dispatch_repository
        .save_dispatch(&dispatch)
        .await
        .expect("terminal dispatch should save");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository.clone(),
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository,
        ),
        &static_root,
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/tasks/{}/dispatch", task.id))
                .body(Body::empty())
                .expect("discard request should build"),
        )
        .await
        .expect("discard request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    assert!(dispatch_repository
        .latest_dispatch_for_task(&task.id)
        .await
        .expect("latest dispatch lookup should succeed")
        .is_none());

    let list_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/dispatches?taskId={}", task.id))
                .body(Body::empty())
                .expect("list request should build"),
        )
        .await
        .expect("list request should succeed");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .expect("list response body should be readable");
    let list_json: serde_json::Value =
        serde_json::from_slice(&list_body).expect("list response should be valid json");
    assert_eq!(list_json["dispatches"].as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn patches_and_deletes_tasks() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let repository = task_repository(&directory).await;
    let created = repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            description: "Update the onboarding guide".to_owned(),
            source: Some(TaskSource::Web),
        })
        .await
        .expect("task should be created");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            repository,
        ),
        &static_root,
    );

    let patch_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/tasks/{}", created.task.id))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"description":"Update the onboarding guide for Linux users","priority":"high","status":"closed"}"#,
                    ))
                    .expect("patch request should build"),
            )
            .await
            .expect("patch request should succeed");
    assert_eq!(patch_response.status(), StatusCode::OK);

    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/tasks/{}", created.task.id))
                .body(Body::empty())
                .expect("delete request should build"),
        )
        .await
        .expect("delete request should succeed");
    assert_eq!(delete_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn bumps_task_change_version_for_notify_and_mutations() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    register_project(&project_repository, "project-a").await;
    let repository = task_repository(&directory).await;
    let created = repository
        .create_task(TaskCreateInput {
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            description: "Versioned task".to_owned(),
            source: Some(TaskSource::Cli),
        })
        .await
        .expect("task should be created");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            repository,
        ),
        &static_root,
    );

    let notify_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events/tasks-changed")
                .body(Body::empty())
                .expect("notify request should build"),
        )
        .await
        .expect("notify request should succeed");
    assert_eq!(notify_response.status(), StatusCode::OK);

    let patch_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/tasks/{}", created.task.id))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"status":"closed"}"#))
                .expect("patch request should build"),
        )
        .await
        .expect("patch request should succeed");
    assert_eq!(patch_response.status(), StatusCode::OK);

    let version_response = app
        .oneshot(
            Request::builder()
                .uri("/api/events/version")
                .body(Body::empty())
                .expect("version request should build"),
        )
        .await
        .expect("version request should succeed");
    assert_eq!(version_response.status(), StatusCode::OK);
    let version_body = axum::body::to_bytes(version_response.into_body(), usize::MAX)
        .await
        .expect("version response body should be readable");
    let version_json: serde_json::Value =
        serde_json::from_slice(&version_body).expect("version response should be valid json");
    assert_eq!(version_json["version"], 2);
}

#[tokio::test]
async fn lists_and_updates_project_metadata() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_path = directory.path().join("workspace/project-a");
    fs::create_dir_all(project_path.join(".git")).expect("git directory should exist");
    fs::write(
        project_path.join(".git/config"),
        "[remote \"origin\"]\n\turl = git@github.com:acme/project-a.git\n",
    )
    .expect("git config should be written");
    let project_repository = project_repository(&directory).await;
    project_repository
        .ensure_project(&ProjectInfo {
            canonical_name: "project-a".to_owned(),
            path: project_path,
            aliases: vec![],
        })
        .await
        .expect("project should initialize");

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository(&directory).await,
        ),
        &static_root,
    );

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/projects")
                .body(Body::empty())
                .expect("list request should build"),
        )
        .await
        .expect("list request should succeed");
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
        .await
        .expect("list response body should be readable");
    let list_json: serde_json::Value =
        serde_json::from_slice(&list_body).expect("list response should be valid json");
    assert_eq!(
        list_json["projects"][0]["metadata"]["repoUrl"],
        "https://github.com/acme/project-a"
    );
    assert_eq!(list_json["projects"][0]["metadata"]["baseBranch"], "main");

    let patch_response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/projects/project-a")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"repoUrl":"https://github.com/acme/project-a","gitUrl":"git@github.com:acme/project-a.git","baseBranch":"release","description":"Release coordination repository."}"#,
                    ))
                    .expect("patch request should build"),
            )
            .await
            .expect("patch request should succeed");
    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_body = axum::body::to_bytes(patch_response.into_body(), usize::MAX)
        .await
        .expect("patch response body should be readable");
    let patch_json: serde_json::Value =
        serde_json::from_slice(&patch_body).expect("patch response should be valid json");
    assert_eq!(patch_json["metadata"]["baseBranch"], "release");
    assert_eq!(
        patch_json["metadata"]["description"],
        "Release coordination repository."
    );
}

#[tokio::test]
async fn rejects_task_creation_for_unknown_projects() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let project_repository = project_repository(&directory).await;
    let task_repository = task_repository(&directory).await;

    let app = build_app(
        app_state(
            config_service(&directory).await,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository,
        ),
        &static_root,
    );

    let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project":"project-a","priority":"medium","description":"This project does not exist yet"}"#,
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response should be valid json");
    assert_eq!(json["error"]["code"], "PROJECT_NOT_FOUND");
}

#[tokio::test]
async fn gets_and_updates_remote_agent_shell_prelude() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let config_service = configured_remote_agent_config_service(&directory).await;
    let project_repository = project_repository(&directory).await;

    let app = build_app(
        app_state(
            config_service,
            dispatch_repository(&directory).await,
            project_repository,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository(&directory).await,
        ),
        &static_root,
    );

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/remote-agent")
                .body(Body::empty())
                .expect("get request should build"),
        )
        .await
        .expect("get request should succeed");
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = axum::body::to_bytes(get_response.into_body(), usize::MAX)
        .await
        .expect("get response body should be readable");
    let get_json: serde_json::Value =
        serde_json::from_slice(&get_body).expect("get response should be valid json");
    assert_eq!(get_json["configured"], true);
    assert_eq!(get_json["preferredTool"], "codex");
    assert_eq!(get_json["shellPrelude"], ". \"$HOME/.cargo/env\"");
    assert_eq!(get_json["reviewFollowUp"]["enabled"], false);

    let patch_response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/remote-agent")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"preferredTool":"claude","shellPrelude":"export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"","reviewFollowUp":{"enabled":true,"mainUser":"octocat","defaultReviewPrompt":"Focus on regressions and missing tests."}}"#,
                    ))
                    .expect("patch request should build"),
            )
            .await
            .expect("patch request should succeed");
    assert_eq!(patch_response.status(), StatusCode::OK);
    let patch_body = axum::body::to_bytes(patch_response.into_body(), usize::MAX)
        .await
        .expect("patch response body should be readable");
    let patch_json: serde_json::Value =
        serde_json::from_slice(&patch_body).expect("patch response should be valid json");
    assert_eq!(patch_json["preferredTool"], "claude");
    assert_eq!(
        patch_json["shellPrelude"],
        "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\""
    );
    assert_eq!(patch_json["reviewFollowUp"]["enabled"], true);
    assert_eq!(patch_json["reviewFollowUp"]["mainUser"], "octocat");
    assert_eq!(
        patch_json["reviewFollowUp"]["defaultReviewPrompt"],
        "Focus on regressions and missing tests."
    );
}

#[tokio::test]
async fn puts_remote_agent_config_for_a_fresh_install() {
    let directory = TempDir::new().expect("tempdir should be created");
    let _environment = TestEnvironment::new(&directory);
    let static_root = static_root(&directory);
    let config_service = config_service(&directory).await;

    let app = build_app(
        app_state(
            config_service,
            dispatch_repository(&directory).await,
            project_repository(&directory).await,
            review_dispatch_repository(&directory).await,
            review_repository(&directory).await,
            task_repository(&directory).await,
        ),
        &static_root,
    );

    let response = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/remote-agent")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"192.0.2.25","user":"builder","port":22,"workspaceRoot":"~/workspace","projectsRegistryPath":"~/track-projects.json","preferredTool":"claude","shellPrelude":"export PATH=\"$HOME/.cargo/bin:$PATH\"","reviewFollowUp":{"enabled":false,"mainUser":"octocat","defaultReviewPrompt":"Focus on regressions."},"sshPrivateKey":"-----BEGIN OPENSSH PRIVATE KEY-----\nkey\n-----END OPENSSH PRIVATE KEY-----\n","knownHosts":"github.com ssh-ed25519 AAAA"}"#,
                    ))
                    .expect("put request should build"),
            )
            .await
            .expect("put request should succeed");
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("put response body should be readable");
    let response_json: serde_json::Value =
        serde_json::from_slice(&response_body).expect("put response should be valid json");
    assert_eq!(response_json["preferredTool"], "claude");

    let key_path =
        get_backend_managed_remote_agent_key_path().expect("managed key path should resolve");
    let known_hosts_path = get_backend_managed_remote_agent_known_hosts_path()
        .expect("managed known_hosts path should resolve");
    assert_eq!(
        fs::read_to_string(&key_path).expect("managed key should be readable"),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nkey\n-----END OPENSSH PRIVATE KEY-----\n"
    );
    assert_eq!(
        fs::read_to_string(&known_hosts_path).expect("known_hosts should be readable"),
        "github.com ssh-ed25519 AAAA"
    );
}
