use std::path::Path;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::services::{ServeDir, ServeFile};
use track_core::config::ConfigService;
use track_core::errors::{ErrorCode, TrackError};
use track_core::project_catalog::ProjectInfo;
use track_core::project_discovery::discover_projects;
use track_core::task_repository::FileTaskRepository;
use track_core::task_sort::sort_tasks;
use track_core::types::{Task, TaskUpdateInput};

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<ConfigService>,
    pub task_repository: Arc<FileTaskRepository>,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Debug, Serialize)]
struct ApiErrorPayload {
    code: String,
    message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    pub fn from_track_error(error: TrackError) -> Self {
        let status = match error.code {
            ErrorCode::TaskNotFound => StatusCode::NOT_FOUND,
            ErrorCode::InvalidJson
            | ErrorCode::InvalidTaskUpdate
            | ErrorCode::ConfigNotFound
            | ErrorCode::InvalidConfig
            | ErrorCode::InvalidConfigInput
            | ErrorCode::NoProjectRoots
            | ErrorCode::NoProjectsDiscovered
            | ErrorCode::InvalidProjectSelection
            | ErrorCode::AiParseFailed
            | ErrorCode::EmptyInput
            | ErrorCode::InteractiveRequired
            | ErrorCode::TaskWriteFailed => StatusCode::BAD_REQUEST,
        };

        Self {
            status,
            code: error.code.to_string(),
            message: error.to_string(),
        }
    }

    pub fn invalid_json(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::InvalidJson.to_string(),
            message: message.to_owned(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_owned(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                error: ApiErrorPayload {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct ProjectsResponse {
    projects: Vec<ProjectInfo>,
}

#[derive(Debug, Serialize)]
struct TasksResponse {
    tasks: Vec<Task>,
}

#[derive(Debug, Serialize)]
struct DeleteTaskResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct TaskListQuery {
    #[serde(rename = "includeClosed")]
    include_closed: Option<bool>,
    project: Option<String>,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<ProjectsResponse>, ApiError> {
    let config = state
        .config_service
        .load_runtime_config()
        .map_err(ApiError::from_track_error)?;
    let projects = discover_projects(&config)
        .map_err(ApiError::from_track_error)?
        .into_projects();

    Ok(Json(ProjectsResponse { projects }))
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<TaskListQuery>,
) -> Result<Json<TasksResponse>, ApiError> {
    let tasks = state
        .task_repository
        .list_tasks(
            query.include_closed.unwrap_or(false),
            query.project.as_deref(),
        )
        .map_err(ApiError::from_track_error)?;

    Ok(Json(TasksResponse {
        tasks: sort_tasks(&tasks),
    }))
}

async fn patch_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<Task>, ApiError> {
    let input = serde_json::from_slice::<TaskUpdateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let validated_input = input.validate().map_err(ApiError::from_track_error)?;

    let updated_task = state
        .task_repository
        .update_task(&id, validated_input)
        .map_err(ApiError::from_track_error)?;

    Ok(Json(updated_task))
}

async fn delete_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    state
        .task_repository
        .delete_task(&id)
        .map_err(ApiError::from_track_error)?;

    Ok(Json(DeleteTaskResponse { ok: true }))
}

async fn api_not_found() -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        code: "ROUTE_NOT_FOUND".to_owned(),
        message: "Route was not found.".to_owned(),
    }
}

pub fn build_app(state: AppState, static_root: impl AsRef<Path>) -> Router {
    // The deployed app still serves both API routes and the frontend from one
    // process so Docker can expose a single local port.
    let static_root = static_root.as_ref().to_path_buf();
    let api_router = Router::new()
        .route("/projects", get(list_projects))
        .route("/tasks", get(list_tasks))
        .route("/tasks/{id}", patch(patch_task).delete(delete_task))
        .fallback(api_not_found);

    Router::new()
        .route("/health", get(health))
        .nest("/api", api_router)
        .fallback_service(
            axum::routing::get_service(
                ServeDir::new(static_root.clone())
                    .not_found_service(ServeFile::new(static_root.join("index.html"))),
            )
            .handle_error(|error| async move {
                ApiError::internal(format!("Static assets are not available yet: {error}"))
            }),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;
    use track_core::config::{ConfigService, LlamaCppConfigFile, TrackConfigFile};
    use track_core::task_repository::FileTaskRepository;
    use track_core::types::{Priority, TaskCreateInput, TaskSource};

    use super::{build_app, AppState};

    fn static_root(directory: &TempDir) -> std::path::PathBuf {
        let root = directory.path().join("static");
        fs::create_dir_all(&root).expect("static root should exist");
        fs::write(root.join("index.html"), "<html><body>track</body></html>")
            .expect("static index should be written");
        root
    }

    #[tokio::test]
    async fn lists_tasks_with_backend_sorting() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("repository should resolve"),
        );
        repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Middle priority task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("first task should be created");
        repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "Top priority task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("second task should be created");

        let app = build_app(
            AppState {
                config_service: Arc::new(
                    ConfigService::new(Some(directory.path().join("missing-config.json")))
                        .expect("config service should resolve"),
                ),
                task_repository: repository,
            },
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
    async fn patches_and_deletes_tasks() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let config_service = ConfigService::new(Some(directory.path().join("config.json")))
            .expect("config service should resolve");
        config_service
            .save_config_file(&TrackConfigFile {
                project_roots: vec![directory.path().join("workspace").display().to_string()],
                project_aliases: Default::default(),
                llama_cpp: LlamaCppConfigFile {
                    model_path: directory.path().join("parser.gguf").display().to_string(),
                    llama_completion_path: None,
                },
            })
            .expect("config should save");
        fs::create_dir_all(directory.path().join("workspace/project-a/.git"))
            .expect("workspace project should exist");

        let repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("repository should resolve"),
        );
        let created = repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Update the onboarding guide".to_owned(),
                source: Some(TaskSource::Web),
            })
            .expect("task should be created");

        let app = build_app(
            AppState {
                config_service: Arc::new(config_service),
                task_repository: repository,
            },
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
}
