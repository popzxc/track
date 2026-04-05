use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;
use track_api::BackendConfigRepository;
use track_api::{build_app, AppState, MigrationService, RemoteAgentConfigService};
use track_dal::database::DatabaseContext;
use track_projects::project_metadata::ProjectMetadata;
use track_types::types::{
    Priority, ReviewRunRecord, Status, Task, TaskCreateInput, TaskSource, TaskUpdateInput,
};

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
    database: DatabaseContext,
    _state_dir: TempDir,
    _track_data_dir_guard: ScopedEnvVar,
    _track_legacy_config_guard: ScopedEnvVar,
    _track_legacy_root_guard: ScopedEnvVar,
    _track_state_dir_guard: ScopedEnvVar,
}

impl ApiHarness {
    pub async fn new(fixture: &RemoteFixture) -> Self {
        let state_dir = TempDir::new().expect("local state tempdir should be created");
        let static_root = create_static_root(&state_dir);
        let track_root = state_dir.path().join("track-root");
        let issues_dir = track_root.join("issues");

        let track_data_dir_guard =
            ScopedEnvVar::set("TRACK_DATA_DIR", issues_dir.to_string_lossy().into_owned());
        let track_state_dir_guard =
            ScopedEnvVar::set("TRACK_STATE_DIR", track_root.to_string_lossy().into_owned());
        let track_legacy_root_guard = ScopedEnvVar::set(
            "TRACK_LEGACY_ROOT",
            state_dir
                .path()
                .join("legacy-root")
                .to_string_lossy()
                .into_owned(),
        );
        let track_legacy_config_guard = ScopedEnvVar::set(
            "TRACK_LEGACY_CONFIG_PATH",
            state_dir
                .path()
                .join("legacy-config/config.json")
                .to_string_lossy()
                .into_owned(),
        );
        let database_path = track_root.join("track.sqlite");

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

        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let config_service = Arc::new(
            RemoteAgentConfigService::new(Some(
                BackendConfigRepository::new(Some(database.clone()))
                    .await
                    .expect("backend config repository should resolve"),
            ))
            .await
            .expect("config service should resolve"),
        );
        config_service
            .save_remote_agent_config(Some(&fixture.remote_agent_config()))
            .await
            .expect("remote-agent config should save");

        let migration_service = Arc::new(
            MigrationService::new((*config_service).clone(), database.clone())
                .expect("migration service should resolve"),
        );

        let app = build_app(
            AppState {
                config_service,
                database: database.clone(),
                migration_service,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
            &static_root,
        );

        Self {
            app,
            database,
            _state_dir: state_dir,
            _track_data_dir_guard: track_data_dir_guard,
            _track_legacy_config_guard: track_legacy_config_guard,
            _track_legacy_root_guard: track_legacy_root_guard,
            _track_state_dir_guard: track_state_dir_guard,
        }
    }

    pub async fn create_task_with_project(
        &self,
        project_name: &str,
        project_metadata: ProjectMetadata,
        task_description: &str,
    ) -> Task {
        self.database
            .project_repository()
            .upsert_project_by_name(project_name, project_metadata, Vec::new())
            .await
            .expect("project metadata should save");

        self.database
            .task_repository()
            .create_task(TaskCreateInput {
                project: project_name.to_owned(),
                priority: Priority::High,
                description: task_description.to_owned(),
                source: Some(TaskSource::Cli),
            })
            .await
            .expect("task should be created")
            .task
    }

    pub async fn dispatch_task(&self, task_id: &str) -> serde_json::Value {
        self.dispatch_task_with_tool(task_id, None).await
    }

    pub async fn dispatch_task_with_tool(
        &self,
        task_id: &str,
        preferred_tool: Option<&str>,
    ) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/tasks/{task_id}/dispatch"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "preferredTool": preferred_tool,
                        }))
                        .expect("dispatch request should serialize"),
                    ))
                    .expect("dispatch request should build"),
            )
            .await
            .expect("dispatch request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn follow_up_task(&self, task_id: &str, request: &str) -> serde_json::Value {
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

    pub async fn update_remote_agent_settings(
        &self,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/remote-agent")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&payload)
                            .expect("remote-agent settings payload should serialize"),
                    ))
                    .expect("remote-agent settings request should build"),
            )
            .await
            .expect("remote-agent settings request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn load_task(&self, task_id: &str) -> Task {
        self.database
            .task_repository()
            .get_task(task_id)
            .await
            .expect("task should load from the repository")
    }

    pub async fn task_exists(&self, task_id: &str) -> bool {
        self.database
            .task_repository()
            .get_task(task_id)
            .await
            .is_ok()
    }

    pub async fn dispatch_history_exists(&self, task_id: &str) -> bool {
        self.database
            .dispatch_repository()
            .latest_dispatch_for_task(task_id)
            .await
            .expect("dispatch history lookup should succeed")
            .is_some()
    }

    pub async fn review_history_exists(&self, review_id: &str) -> bool {
        self.database
            .review_dispatch_repository()
            .latest_dispatch_for_review(review_id)
            .await
            .expect("review history lookup should succeed")
            .is_some()
    }

    pub async fn review_record_exists(&self, review_id: &str) -> bool {
        self.database
            .review_repository()
            .get_review(review_id)
            .await
            .is_ok()
    }

    pub async fn create_review(
        &self,
        pull_request_url: &str,
        extra_instructions: Option<&str>,
    ) -> serde_json::Value {
        self.create_review_with_tool(pull_request_url, extra_instructions, None)
            .await
    }

    pub async fn create_review_with_tool(
        &self,
        pull_request_url: &str,
        extra_instructions: Option<&str>,
        preferred_tool: Option<&str>,
    ) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/reviews")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "pullRequestUrl": pull_request_url,
                            "preferredTool": preferred_tool,
                            "extraInstructions": extra_instructions,
                        }))
                        .expect("review request should serialize"),
                    ))
                    .expect("review request should build"),
            )
            .await
            .expect("review request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn delete_review(&self, review_id: &str) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/reviews/{review_id}"))
                    .body(Body::empty())
                    .expect("review delete request should build"),
            )
            .await
            .expect("review delete request should succeed");
        assert_eq!(response.status(), StatusCode::OK);
    }

    pub async fn follow_up_review(&self, review_id: &str, request: &str) -> serde_json::Value {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/reviews/{review_id}/follow-up"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({ "request": request }))
                            .expect("review follow-up request should serialize"),
                    ))
                    .expect("review follow-up request should build"),
            )
            .await
            .expect("review follow-up request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        response_json(response).await
    }

    pub async fn poll_review_until_terminal(
        &self,
        review_id: &str,
        timeout: Duration,
    ) -> serde_json::Value {
        let deadline = Instant::now() + timeout;
        loop {
            let run = self.latest_review_run(review_id).await;
            let status = run["status"]
                .as_str()
                .expect("review run should contain a status");
            if status != "preparing" && status != "running" {
                return run;
            }

            assert!(
                Instant::now() < deadline,
                "review did not reach a terminal state before the timeout.\nlast review run:\n{}",
                serde_json::to_string_pretty(&run).expect("review run JSON should serialize")
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    async fn latest_review_run(&self, review_id: &str) -> serde_json::Value {
        self.review_runs(review_id)
            .await
            .into_iter()
            .next()
            .expect("review runs response should include at least one run")
    }

    pub async fn review_runs(&self, review_id: &str) -> Vec<serde_json::Value> {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/reviews/{review_id}/runs"))
                    .body(Body::empty())
                    .expect("review runs request should build"),
            )
            .await
            .expect("review runs request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        let json = response_json(response).await;
        json["runs"]
            .as_array()
            .cloned()
            .expect("review runs response should include a runs array")
    }

    pub async fn cancel_review(&self, review_id: &str) -> ReviewRunRecord {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/reviews/{review_id}/cancel"))
                    .body(Body::empty())
                    .expect("review cancel request should build"),
            )
            .await
            .expect("review cancel request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        serde_json::from_value(response_json(response).await)
            .expect("review cancel response should deserialize")
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

    pub async fn close_task_without_remote_cleanup(&self, task_id: &str) -> Task {
        self.database
            .task_repository()
            .update_task(
                task_id,
                TaskUpdateInput {
                    description: None,
                    priority: None,
                    status: Some(Status::Closed),
                },
            )
            .await
            .expect("task should close directly in the repository")
    }

    pub async fn delete_task_file_without_remote_cleanup(&self, task_id: &str) {
        self.database
            .task_repository()
            .delete_task(task_id)
            .await
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
