use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;
use track_api::{AppState, build_app};
use track_core::config::{
    ApiConfigFile, ConfigService, LlamaCppConfigFile, TrackConfigFile,
};
use track_core::dispatch_repository::DispatchRepository;
use track_core::project_repository::{ProjectMetadata, ProjectRepository};
use track_core::task_repository::FileTaskRepository;
use track_core::types::{Priority, Status, Task, TaskCreateInput, TaskSource, TaskUpdateInput};

use crate::fixture::RemoteFixture;

// =============================================================================
// In-Process API Harness
// =============================================================================
//
// The live integration tests want real SSH and real background dispatch logic,
// but they do not need a separately spawned HTTP server process yet. This
// harness keeps the API in-process so each test can control local track state
// precisely while still exercising the production router and background work.
pub struct ApiHarness {
    app: axum::Router,
    project_repository: Arc<ProjectRepository>,
    task_repository: Arc<FileTaskRepository>,
    dispatches_dir: PathBuf,
    _state_dir: TempDir,
    _track_data_dir_guard: ScopedEnvVar,
}

impl ApiHarness {
    pub fn new(fixture: &RemoteFixture) -> Self {
        let state_dir = TempDir::new().expect("local state tempdir should be created");
        let static_root = create_static_root(&state_dir);
        let issues_dir = state_dir.path().join("track-root/issues");
        let config_path = state_dir.path().join("config/config.json");

        let track_data_dir_guard =
            ScopedEnvVar::set("TRACK_DATA_DIR", issues_dir.to_string_lossy().into_owned());

        let managed_remote_agent_dir = issues_dir
            .parent()
            .expect("issues directory should have a parent")
            .join("remote-agent");
        fs::create_dir_all(&managed_remote_agent_dir)
            .expect("managed remote-agent directory should exist");
        fs::copy(
            fixture.private_key_path(),
            managed_remote_agent_dir.join("id_ed25519"),
        )
        .expect("managed SSH key should copy into the local track state");
        set_private_key_permissions(&managed_remote_agent_dir.join("id_ed25519"));

        let config_service = Arc::new(
            ConfigService::new(Some(config_path)).expect("config service should resolve"),
        );
        config_service
            .save_config_file(&TrackConfigFile {
                project_roots: vec![],
                project_aliases: Default::default(),
                api: ApiConfigFile::default(),
                llama_cpp: LlamaCppConfigFile {
                    model_path: "/tmp/model.gguf".to_owned(),
                    llama_completion_path: None,
                },
                remote_agent: Some(fixture.remote_agent_config()),
            })
            .expect("remote-agent config should save");

        let task_repository = Arc::new(
            FileTaskRepository::new(Some(issues_dir.clone()))
                .expect("task repository should resolve"),
        );
        let dispatch_repository = Arc::new(
            DispatchRepository::new(Some(issues_dir.join(".dispatches")))
                .expect("dispatch repository should resolve"),
        );
        let project_repository = Arc::new(
            ProjectRepository::new(Some(issues_dir.clone()))
                .expect("project repository should resolve"),
        );

        let app = build_app(
            AppState {
                config_service,
                dispatch_repository,
                project_repository: project_repository.clone(),
                task_repository: task_repository.clone(),
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
            &static_root,
        );

        Self {
            app,
            project_repository,
            task_repository,
            dispatches_dir: issues_dir.join(".dispatches"),
            _state_dir: state_dir,
            _track_data_dir_guard: track_data_dir_guard,
        }
    }

    pub fn create_task_with_project(
        &self,
        project_name: &str,
        project_metadata: ProjectMetadata,
        task_description: &str,
    ) -> Task {
        let task = self
            .task_repository
            .create_task(TaskCreateInput {
                project: project_name.to_owned(),
                priority: Priority::High,
                description: task_description.to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created")
            .task;

        // Project metadata lives alongside the project's task directories, so
        // we create the first task before attempting to persist PROJECT.md.
        self.project_repository
            .update_project_by_name(project_name, project_metadata)
            .expect("project metadata should update");

        task
    }

    pub async fn dispatch_task(&self, task_id: &str) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{task_id}/dispatch"))
                    .body(Body::empty())
                    .expect("dispatch request should build"),
            )
            .await
            .expect("dispatch request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn follow_up_task(
        &self,
        task_id: &str,
        request: &str,
    ) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{task_id}/follow-up"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({ "request": request }))
                            .expect("follow-up request should serialize"),
                    ))
                    .expect("follow-up request should build"),
            )
            .await
            .expect("follow-up request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub fn load_task(&self, task_id: &str) -> Task {
        self.task_repository
            .get_task(task_id)
            .expect("task should load from the repository")
    }

    pub fn task_exists(&self, task_id: &str) -> bool {
        self.task_repository.get_task(task_id).is_ok()
    }

    pub fn dispatch_history_exists(&self, task_id: &str) -> bool {
        self.dispatches_dir.join(task_id).exists()
    }

    pub async fn update_task_status(&self, task_id: &str, status: &str) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/tasks/{task_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({ "status": status }))
                            .expect("task status update should serialize"),
                    ))
                    .expect("task status update request should build"),
            )
            .await
            .expect("task status update request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn delete_task(&self, task_id: &str) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .expect("task delete request should build"),
            )
            .await
            .expect("task delete request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
    }

    pub async fn cleanup_remote_agent_artifacts(&self) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/remote-agent/cleanup")
                    .body(Body::empty())
                    .expect("remote cleanup request should build"),
            )
            .await
            .expect("remote cleanup request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn reset_remote_agent_workspace(&self) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/remote-agent/reset")
                    .body(Body::empty())
                    .expect("remote reset request should build"),
            )
            .await
            .expect("remote reset request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn reset_remote_agent_workspace_expect_error(
        &self,
        expected_status: StatusCode,
    ) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/remote-agent/reset")
                    .body(Body::empty())
                    .expect("remote reset request should build"),
            )
            .await
            .expect("remote reset request should succeed");
        assert_eq!(response.status(), expected_status);

        response_json(response).await
    }

    pub fn close_task_without_remote_cleanup(&self, task_id: &str) -> Task {
        self.task_repository
            .update_task(
                task_id,
                TaskUpdateInput {
                    description: None,
                    priority: None,
                    status: Some(Status::Closed),
                },
            )
            .expect("task should close directly in the repository")
    }

    pub fn delete_task_file_without_remote_cleanup(&self, task_id: &str) {
        self.task_repository
            .delete_task(task_id)
            .expect("task file should delete directly in the repository");
    }

    pub async fn poll_dispatch_until_terminal(
        &self,
        task_id: &str,
        timeout: Duration,
    ) -> serde_json::Value {
        self.poll_dispatches_until_all_terminal(&[task_id.to_owned()], timeout)
            .await
            .remove(task_id)
            .expect("task should have a terminal dispatch")
    }

    pub async fn poll_dispatches_until_all_terminal(
        &self,
        task_ids: &[String],
        timeout: Duration,
    ) -> BTreeMap<String, serde_json::Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let dispatches = self.list_dispatches(task_ids).await;
            let all_terminal = task_ids.iter().all(|task_id| {
                dispatches
                    .get(task_id)
                    .and_then(|dispatch| dispatch["status"].as_str())
                    .map(|status| status != "preparing" && status != "running")
                    .unwrap_or(false)
            });
            if all_terminal {
                return dispatches;
            }

            assert!(
                Instant::now() < deadline,
                "dispatches did not reach a terminal state before the timeout.\nlast dispatch set:\n{}",
                serde_json::to_string_pretty(&dispatches)
                    .expect("dispatch JSON should serialize")
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    async fn list_dispatches(&self, task_ids: &[String]) -> BTreeMap<String, serde_json::Value> {
        let query = task_ids
            .iter()
            .map(|task_id| format!("taskId={task_id}"))
            .collect::<Vec<_>>()
            .join("&");
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/dispatches?{query}"))
                    .body(Body::empty())
                    .expect("dispatch list request should build"),
            )
            .await
            .expect("dispatch list request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        let json = response_json(response).await;
        let mut by_task_id = BTreeMap::new();
        for dispatch in json["dispatches"]
            .as_array()
            .expect("dispatch list should be an array")
        {
            let task_id = dispatch["taskId"]
                .as_str()
                .expect("dispatch should contain taskId")
                .to_owned();
            by_task_id.insert(task_id, dispatch.clone());
        }

        by_task_id
    }
}

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&body).expect("response body should be valid JSON")
}

fn create_static_root(directory: &TempDir) -> PathBuf {
    let root = directory.path().join("static");
    fs::create_dir_all(&root).expect("static root should exist");

    // The integration harness is about backend behavior, so a minimal static
    // root is enough to satisfy the production router's asset fallback.
    fs::write(root.join("index.html"), "<html><body>track</body></html>")
        .expect("static index should be written");
    root
}

fn set_private_key_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions).expect("private key permissions should update");
    }
}

struct ScopedEnvVar {
    key: &'static str,
    original: Option<String>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.original.as_deref() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
