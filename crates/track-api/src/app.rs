use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{StatusCode, Uri};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{extract::Request, response::Response as AxumResponse};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::services::{ServeDir, ServeFile};
use track_core::backend_config::RemoteAgentConfigService;
use track_core::build_info::BuildInfo;
use track_core::config::{
    RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile, DEFAULT_REMOTE_AGENT_PORT,
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT, DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use track_core::dispatch_repository::DispatchRepository;
use track_core::errors::{ErrorCode, TrackError};
use track_core::migration::{MigrationImportSummary, MigrationStatus};
use track_core::migration_service::MigrationService;
use track_core::project_repository::{
    ProjectMetadataUpdateInput, ProjectRecord, ProjectRepository, ProjectUpsertInput,
};
use track_core::remote_agent::{RemoteDispatchService, RemoteReviewService};
use track_core::review_dispatch_repository::ReviewDispatchRepository;
use track_core::review_repository::ReviewRepository;
use track_core::task_repository::FileTaskRepository;
use track_core::task_sort::sort_tasks;
use track_core::time_utils::now_utc;
use track_core::types::{
    CreateReviewInput, RemoteAgentPreferredTool, RemoteCleanupSummary, RemoteResetSummary,
    ReviewRecord, ReviewRunRecord, Task, TaskCreateInput, TaskDispatchRecord, TaskSource,
    TaskUpdateInput,
};

use crate::build_info::server_build_info;

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<RemoteAgentConfigService>,
    pub dispatch_repository: Arc<DispatchRepository>,
    pub migration_service: Arc<MigrationService>,
    pub project_repository: Arc<ProjectRepository>,
    pub review_dispatch_repository: Arc<ReviewDispatchRepository>,
    pub review_repository: Arc<ReviewRepository>,
    pub task_repository: Arc<FileTaskRepository>,
    pub task_change_version: Arc<AtomicU64>,
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
            | ErrorCode::InvalidPathComponent
            | ErrorCode::InvalidProjectMetadata
            | ErrorCode::InvalidRemoteAgentConfig
            | ErrorCode::InvalidTaskUpdate
            | ErrorCode::VersionMismatch
            | ErrorCode::MigrationRequired
            | ErrorCode::ConfigNotFound
            | ErrorCode::InvalidConfig
            | ErrorCode::InvalidConfigInput
            | ErrorCode::NoProjectRoots
            | ErrorCode::NoProjectsDiscovered
            | ErrorCode::InvalidProjectSelection
            | ErrorCode::AiParseFailed
            | ErrorCode::EmptyInput
            | ErrorCode::InteractiveRequired
            | ErrorCode::DispatchWriteFailed
            | ErrorCode::RemoteAgentNotConfigured
            | ErrorCode::ProjectWriteFailed
            | ErrorCode::TaskWriteFailed => StatusCode::BAD_REQUEST,
            ErrorCode::MigrationFailed => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::ProjectNotFound | ErrorCode::DispatchNotFound => StatusCode::NOT_FOUND,
            ErrorCode::RemoteDispatchFailed => StatusCode::BAD_GATEWAY,
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
    projects: Vec<ProjectRecord>,
}

#[derive(Debug, Serialize)]
struct TasksResponse {
    tasks: Vec<Task>,
}

#[derive(Debug, Serialize)]
struct DeleteTaskResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct DispatchesResponse {
    dispatches: Vec<TaskDispatchRecord>,
}

#[derive(Debug, Serialize)]
struct RunRecordResponse {
    task: Task,
    dispatch: TaskDispatchRecord,
}

#[derive(Debug, Serialize)]
struct RunsResponse {
    runs: Vec<RunRecordResponse>,
}

#[derive(Debug, Serialize)]
struct ReviewSummaryResponse {
    review: ReviewRecord,
    #[serde(rename = "latestRun", skip_serializing_if = "Option::is_none")]
    latest_run: Option<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
struct ReviewsResponse {
    reviews: Vec<ReviewSummaryResponse>,
}

#[derive(Debug, Serialize)]
struct ReviewRunsResponse {
    runs: Vec<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
struct CreateReviewResponse {
    review: ReviewRecord,
    run: ReviewRunRecord,
}

#[derive(Debug, Serialize)]
struct RemoteAgentSettingsResponse {
    configured: bool,
    #[serde(rename = "preferredTool")]
    preferred_tool: RemoteAgentPreferredTool,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(rename = "shellPrelude", skip_serializing_if = "Option::is_none")]
    shell_prelude: Option<String>,
    #[serde(rename = "reviewFollowUp", skip_serializing_if = "Option::is_none")]
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteAgentReviewFollowUpSettingsResponse {
    enabled: bool,
    #[serde(rename = "mainUser", skip_serializing_if = "Option::is_none")]
    main_user: Option<String>,
    #[serde(
        rename = "defaultReviewPrompt",
        skip_serializing_if = "Option::is_none"
    )]
    default_review_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
struct RemoteCleanupResponse {
    summary: RemoteCleanupSummary,
}

#[derive(Debug, Serialize)]
struct RemoteResetResponse {
    summary: RemoteResetSummary,
}

#[derive(Debug, Serialize)]
struct TaskChangeVersionResponse {
    version: u64,
}

#[derive(Debug, Serialize)]
struct MigrationStatusResponse {
    migration: MigrationStatus,
}

#[derive(Debug, Serialize)]
struct MigrationImportResponse {
    summary: MigrationImportSummary,
}

#[derive(Debug, Deserialize)]
struct TaskListQuery {
    #[serde(rename = "includeClosed")]
    include_closed: Option<bool>,
    project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct UpdateRemoteAgentSettingsInput {
    #[serde(rename = "preferredTool", default)]
    preferred_tool: Option<RemoteAgentPreferredTool>,
    #[serde(rename = "shellPrelude")]
    shell_prelude: String,
    #[serde(rename = "reviewFollowUp")]
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
}

#[derive(Debug, Deserialize)]
struct PutRemoteAgentInput {
    host: String,
    user: String,
    #[serde(default = "default_remote_agent_port")]
    port: u16,
    #[serde(
        rename = "workspaceRoot",
        default = "default_remote_agent_workspace_root"
    )]
    workspace_root: String,
    #[serde(
        rename = "projectsRegistryPath",
        default = "default_remote_projects_registry_path"
    )]
    projects_registry_path: String,
    #[serde(rename = "preferredTool", default)]
    preferred_tool: RemoteAgentPreferredTool,
    #[serde(rename = "shellPrelude")]
    shell_prelude: Option<String>,
    #[serde(rename = "reviewFollowUp")]
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
    #[serde(rename = "sshPrivateKey")]
    ssh_private_key: String,
    #[serde(rename = "knownHosts")]
    known_hosts: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PutProjectInput {
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(flatten)]
    metadata: ProjectMetadataUpdateInput,
}

#[derive(Debug, Deserialize)]
struct FollowUpRequestInput {
    request: String,
}

#[derive(Debug, Default, Deserialize)]
struct DispatchTaskInput {
    #[serde(rename = "preferredTool", default)]
    preferred_tool: Option<RemoteAgentPreferredTool>,
}

fn remote_agent_settings_response(
    remote_agent: Option<RemoteAgentConfigFile>,
) -> RemoteAgentSettingsResponse {
    match remote_agent {
        Some(remote_agent) => RemoteAgentSettingsResponse {
            configured: true,
            preferred_tool: remote_agent.preferred_tool,
            host: Some(remote_agent.host),
            user: Some(remote_agent.user),
            port: Some(remote_agent.port),
            shell_prelude: remote_agent.shell_prelude,
            review_follow_up: Some(
                remote_agent
                    .review_follow_up
                    .map(
                        |review_follow_up| RemoteAgentReviewFollowUpSettingsResponse {
                            enabled: review_follow_up.enabled,
                            main_user: review_follow_up.main_user,
                            default_review_prompt: review_follow_up.default_review_prompt,
                        },
                    )
                    .unwrap_or(RemoteAgentReviewFollowUpSettingsResponse {
                        enabled: false,
                        main_user: None,
                        default_review_prompt: None,
                    }),
            ),
        },
        None => RemoteAgentSettingsResponse {
            configured: false,
            preferred_tool: RemoteAgentPreferredTool::Codex,
            host: None,
            user: None,
            port: None,
            shell_prelude: None,
            review_follow_up: None,
        },
    }
}

fn default_remote_agent_port() -> u16 {
    DEFAULT_REMOTE_AGENT_PORT
}

fn default_remote_agent_workspace_root() -> String {
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned()
}

fn default_remote_projects_registry_path() -> String {
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned()
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn get_server_version() -> Json<BuildInfo> {
    Json(server_build_info())
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<ProjectsResponse>, ApiError> {
    let projects = state
        .project_repository
        .list_projects()
        .map_err(ApiError::from_track_error)?;

    Ok(Json(ProjectsResponse { projects }))
}

async fn migration_status(
    State(state): State<AppState>,
) -> Result<Json<MigrationStatusResponse>, ApiError> {
    let migration = state
        .migration_service
        .status()
        .map_err(ApiError::from_track_error)?;

    Ok(Json(MigrationStatusResponse { migration }))
}

async fn import_legacy_data(
    State(state): State<AppState>,
) -> Result<Json<MigrationImportResponse>, ApiError> {
    let summary = state
        .migration_service
        .import_legacy()
        .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    Ok(Json(MigrationImportResponse { summary }))
}

async fn get_remote_agent_settings(
    State(state): State<AppState>,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let remote_agent = state
        .config_service
        .load_remote_agent_config()
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(remote_agent)))
}

async fn patch_remote_agent_settings(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let input = serde_json::from_slice::<UpdateRemoteAgentSettingsInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let existing_remote_agent = state
        .config_service
        .load_remote_agent_config()
        .map_err(ApiError::from_track_error)?
        .ok_or_else(|| ApiError::from_track_error(TrackError::new(
            ErrorCode::RemoteAgentNotConfigured,
            "Remote dispatch is not configured yet. Run `track remote-agent configure ...` locally to register the remote host and SSH key first.",
        )))?;

    let remote_agent = state
        .config_service
        .save_remote_agent_settings(
            input
                .preferred_tool
                .unwrap_or(existing_remote_agent.preferred_tool),
            Some(input.shell_prelude),
            input
                .review_follow_up
                .map(|review_follow_up| RemoteAgentReviewFollowUpConfigFile {
                    enabled: review_follow_up.enabled,
                    main_user: review_follow_up.main_user,
                    default_review_prompt: review_follow_up.default_review_prompt,
                })
                .or(existing_remote_agent.review_follow_up),
        )
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(Some(remote_agent))))
}

async fn put_remote_agent_settings(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let input = serde_json::from_slice::<PutRemoteAgentInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let remote_agent = state
        .config_service
        .replace_remote_agent_config(
            RemoteAgentConfigFile {
                host: input.host,
                user: input.user,
                port: input.port,
                workspace_root: input.workspace_root,
                projects_registry_path: input.projects_registry_path,
                preferred_tool: input.preferred_tool,
                shell_prelude: input.shell_prelude,
                review_follow_up: input.review_follow_up.map(|review_follow_up| {
                    RemoteAgentReviewFollowUpConfigFile {
                        enabled: review_follow_up.enabled,
                        main_user: review_follow_up.main_user,
                        default_review_prompt: review_follow_up.default_review_prompt,
                    }
                }),
            },
            &input.ssh_private_key,
            input.known_hosts.as_deref(),
        )
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(Some(remote_agent))))
}

async fn cleanup_remote_agent_artifacts(
    State(state): State<AppState>,
) -> Result<Json<RemoteCleanupResponse>, ApiError> {
    let cleanup_state = state.clone();
    let summary = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &cleanup_state.config_service,
            dispatch_repository: &cleanup_state.dispatch_repository,
            project_repository: &cleanup_state.project_repository,
            task_repository: &cleanup_state.task_repository,
            review_repository: &cleanup_state.review_repository,
            review_dispatch_repository: &cleanup_state.review_dispatch_repository,
        };

        dispatch_service.cleanup_unused_remote_artifacts()
    })
    .await
    .map_err(|error| ApiError::internal(format!("Remote cleanup task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(RemoteCleanupResponse { summary }))
}

async fn reset_remote_agent_workspace(
    State(state): State<AppState>,
) -> Result<Json<RemoteResetResponse>, ApiError> {
    let reset_state = state.clone();
    let summary = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &reset_state.config_service,
            dispatch_repository: &reset_state.dispatch_repository,
            project_repository: &reset_state.project_repository,
            task_repository: &reset_state.task_repository,
            review_repository: &reset_state.review_repository,
            review_dispatch_repository: &reset_state.review_dispatch_repository,
        };

        dispatch_service.reset_remote_workspace()
    })
    .await
    .map_err(|error| ApiError::internal(format!("Remote reset task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(RemoteResetResponse { summary }))
}

async fn patch_project(
    State(state): State<AppState>,
    AxumPath(canonical_name): AxumPath<String>,
    body: Bytes,
) -> Result<Json<ProjectRecord>, ApiError> {
    let input = serde_json::from_slice::<ProjectMetadataUpdateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let project = state
        .project_repository
        .update_project_by_name(
            &canonical_name,
            input.validate().map_err(ApiError::from_track_error)?,
        )
        .map_err(ApiError::from_track_error)?;

    Ok(Json(project))
}

async fn put_project(
    State(state): State<AppState>,
    AxumPath(canonical_name): AxumPath<String>,
    body: Bytes,
) -> Result<Json<ProjectRecord>, ApiError> {
    let input = serde_json::from_slice::<PutProjectInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let project = state
        .project_repository
        .upsert_project(ProjectUpsertInput {
            canonical_name,
            aliases: input.aliases,
            metadata: input.metadata,
        })
        .map_err(ApiError::from_track_error)?;

    Ok(Json(project))
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

async fn list_dispatches(
    State(state): State<AppState>,
    uri: Uri,
) -> Result<Json<DispatchesResponse>, ApiError> {
    let state = state.clone();
    let task_ids = parse_dispatch_task_ids(uri.query());
    let dispatches = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &state.config_service,
            dispatch_repository: &state.dispatch_repository,
            project_repository: &state.project_repository,
            task_repository: &state.task_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        dispatch_service.latest_dispatches_for_tasks(&task_ids)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Dispatch refresh task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(DispatchesResponse { dispatches }))
}

async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let state = state.clone();
    let limit = query.limit;
    let runs = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &state.config_service,
            dispatch_repository: &state.dispatch_repository,
            project_repository: &state.project_repository,
            task_repository: &state.task_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        let dispatches = dispatch_service.list_dispatches(limit)?;
        let mut runs = Vec::new();
        for dispatch in dispatches {
            let task = match state.task_repository.get_task(&dispatch.task_id) {
                Ok(task) => task,
                // Runs are persisted separately from task files. If a task was
                // deleted later, we prefer to hide that orphaned run from the
                // normal UI instead of turning the whole page into an error.
                Err(error) if error.code == ErrorCode::TaskNotFound => continue,
                Err(error) => return Err(error),
            };
            runs.push(RunRecordResponse { task, dispatch });
        }

        Ok::<Vec<RunRecordResponse>, TrackError>(runs)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Runs refresh task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(RunsResponse { runs }))
}

async fn list_task_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<RunsResponse>, ApiError> {
    let state = state.clone();
    let task_id = id.clone();
    let runs = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &state.config_service,
            dispatch_repository: &state.dispatch_repository,
            project_repository: &state.project_repository,
            task_repository: &state.task_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        let task = state.task_repository.get_task(&task_id)?;
        let dispatches = dispatch_service.dispatch_history_for_task(&task_id)?;

        Ok::<Vec<RunRecordResponse>, TrackError>(
            dispatches
                .into_iter()
                .map(|dispatch| RunRecordResponse {
                    task: task.clone(),
                    dispatch,
                })
                .collect(),
        )
    })
    .await
    .map_err(|error| ApiError::internal(format!("Task runs refresh failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(RunsResponse { runs }))
}

async fn list_reviews(State(state): State<AppState>) -> Result<Json<ReviewsResponse>, ApiError> {
    let state = state.clone();
    let reviews = tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &state.config_service,
            project_repository: &state.project_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        let reviews = state.review_repository.list_reviews()?;
        let review_ids = reviews
            .iter()
            .map(|review| review.id.clone())
            .collect::<Vec<_>>();
        let latest_runs = review_service.latest_dispatches_for_reviews(&review_ids)?;
        let latest_runs_by_review_id = latest_runs
            .into_iter()
            .map(|run| (run.review_id.clone(), run))
            .collect::<std::collections::BTreeMap<_, _>>();

        Ok::<Vec<ReviewSummaryResponse>, TrackError>(
            reviews
                .into_iter()
                .map(|review| ReviewSummaryResponse {
                    latest_run: latest_runs_by_review_id.get(&review.id).cloned(),
                    review,
                })
                .collect(),
        )
    })
    .await
    .map_err(|error| ApiError::internal(format!("Review list refresh failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(ReviewsResponse { reviews }))
}

async fn list_review_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<ReviewRunsResponse>, ApiError> {
    let state = state.clone();
    let review_id = id.clone();
    let runs = tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &state.config_service,
            project_repository: &state.project_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        review_service.dispatch_history_for_review(&review_id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Review runs refresh failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(ReviewRunsResponse { runs }))
}

// =============================================================================
// Dispatch Query Parsing
// =============================================================================
//
// The frontend sends `/api/dispatches?taskId=...&taskId=...` so the browser can
// ask for many task rows in one request. Axum's serde-based query extractor is
// strict here and rejects a plain repeated scalar as "expected a sequence", so
// we parse the raw query ourselves instead of making the UI change shape.
//
// Task ids are filesystem-derived slugs, so we intentionally keep this parser
// narrow and only extract repeated `taskId=` entries. A full percent-decoding
// query parser would add complexity without buying us anything for this domain.
// TODO: Expand this helper if dispatch lookups ever need arbitrary free-form ids.
fn parse_dispatch_task_ids(raw_query: Option<&str>) -> Vec<String> {
    let Some(raw_query) = raw_query else {
        return Vec::new();
    };

    raw_query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key != "taskId" || value.is_empty() {
                return None;
            }

            Some(value.to_owned())
        })
        .collect()
}

fn bump_task_change_version(state: &AppState) -> u64 {
    state.task_change_version.fetch_add(1, Ordering::SeqCst) + 1
}

async fn get_task_change_version(State(state): State<AppState>) -> Json<TaskChangeVersionResponse> {
    Json(TaskChangeVersionResponse {
        version: state.task_change_version.load(Ordering::SeqCst),
    })
}

async fn notify_task_change(State(state): State<AppState>) -> Json<TaskChangeVersionResponse> {
    Json(TaskChangeVersionResponse {
        version: bump_task_change_version(&state),
    })
}

async fn patch_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<Task>, ApiError> {
    let input = serde_json::from_slice::<TaskUpdateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let validated_input = input.validate().map_err(ApiError::from_track_error)?;

    let patch_state = state.clone();
    let task_id = id.clone();
    let updated_task = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &patch_state.config_service,
            dispatch_repository: &patch_state.dispatch_repository,
            project_repository: &patch_state.project_repository,
            task_repository: &patch_state.task_repository,
            review_repository: &patch_state.review_repository,
            review_dispatch_repository: &patch_state.review_dispatch_repository,
        };

        dispatch_service.update_task(&task_id, validated_input)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Patch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    Ok(Json(updated_task))
}

async fn create_task(State(state): State<AppState>, body: Bytes) -> Result<Json<Task>, ApiError> {
    let input = serde_json::from_slice::<TaskCreateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let TaskCreateInput {
        project,
        priority,
        description,
        source,
    } = input;
    let validated_input = TaskCreateInput {
        project,
        priority,
        description,
        source: source.or(Some(TaskSource::Web)),
    }
    .validate()
    .map_err(ApiError::from_track_error)?;

    let created_task = state
        .task_repository
        .create_task(validated_input)
        .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    Ok(Json(created_task.task))
}

async fn delete_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    let delete_state = state.clone();
    let task_id = id.clone();
    tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &delete_state.config_service,
            dispatch_repository: &delete_state.dispatch_repository,
            project_repository: &delete_state.project_repository,
            task_repository: &delete_state.task_repository,
            review_repository: &delete_state.review_repository,
            review_dispatch_repository: &delete_state.review_dispatch_repository,
        };

        dispatch_service.delete_task(&task_id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Delete task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    Ok(Json(DeleteTaskResponse { ok: true }))
}

async fn create_review(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<CreateReviewResponse>, ApiError> {
    let input = serde_json::from_slice::<CreateReviewInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let queue_state = state.clone();
    let (review, run) = tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &queue_state.config_service,
            project_repository: &queue_state.project_repository,
            review_repository: &queue_state.review_repository,
            review_dispatch_repository: &queue_state.review_dispatch_repository,
        };

        review_service.create_review(input)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Create review failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());

    Ok(Json(CreateReviewResponse { review, run }))
}

async fn follow_up_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let queue_state = state.clone();
    let review_id = id.clone();
    let run = tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &queue_state.config_service,
            project_repository: &queue_state.project_repository,
            review_repository: &queue_state.review_repository,
            review_dispatch_repository: &queue_state.review_dispatch_repository,
        };

        review_service.queue_follow_up_review_dispatch(&review_id, &input.request)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Follow-up review failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());

    Ok(Json(run))
}

async fn delete_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    let delete_state = state.clone();
    let review_id = id.clone();
    tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &delete_state.config_service,
            project_repository: &delete_state.project_repository,
            review_repository: &delete_state.review_repository,
            review_dispatch_repository: &delete_state.review_dispatch_repository,
        };

        review_service.delete_review(&review_id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Delete review failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    Ok(Json(DeleteTaskResponse { ok: true }))
}

fn spawn_dispatch_launch(state: AppState, queued_dispatch: TaskDispatchRecord) {
    tokio::spawn(async move {
        let launch_state = state.clone();
        let launch_dispatch = queued_dispatch.clone();
        let join_result = tokio::task::spawn_blocking(move || {
            let dispatch_service = RemoteDispatchService {
                config_service: &launch_state.config_service,
                dispatch_repository: &launch_state.dispatch_repository,
                project_repository: &launch_state.project_repository,
                task_repository: &launch_state.task_repository,
                review_repository: &launch_state.review_repository,
                review_dispatch_repository: &launch_state.review_dispatch_repository,
            };

            dispatch_service.launch_prepared_dispatch(launch_dispatch)
        })
        .await;

        if let Err(join_error) = join_result {
            if let Some(mut saved_dispatch) = state
                .dispatch_repository
                .get_dispatch(&queued_dispatch.task_id, &queued_dispatch.dispatch_id)
                .ok()
                .flatten()
            {
                if saved_dispatch.status.is_active() {
                    saved_dispatch.status = track_core::types::DispatchStatus::Failed;
                    saved_dispatch.updated_at = now_utc();
                    saved_dispatch.finished_at = Some(saved_dispatch.updated_at);
                    saved_dispatch.error_message = Some(format!(
                        "Background dispatch task stopped unexpectedly: {join_error}"
                    ));
                    let _ = state.dispatch_repository.save_dispatch(&saved_dispatch);
                }
            }
        }
    });
}

fn spawn_review_launch(state: AppState, queued_dispatch: ReviewRunRecord) {
    tokio::spawn(async move {
        let launch_state = state.clone();
        let launch_dispatch = queued_dispatch.clone();
        let join_result = tokio::task::spawn_blocking(move || {
            let review_service = RemoteReviewService {
                config_service: &launch_state.config_service,
                project_repository: &launch_state.project_repository,
                review_repository: &launch_state.review_repository,
                review_dispatch_repository: &launch_state.review_dispatch_repository,
            };

            review_service.launch_prepared_review(launch_dispatch)
        })
        .await;

        if let Err(join_error) = join_result {
            if let Some(mut saved_dispatch) = state
                .review_dispatch_repository
                .get_dispatch(&queued_dispatch.review_id, &queued_dispatch.dispatch_id)
                .ok()
                .flatten()
            {
                if saved_dispatch.status.is_active() {
                    saved_dispatch.status = track_core::types::DispatchStatus::Failed;
                    saved_dispatch.updated_at = now_utc();
                    saved_dispatch.finished_at = Some(saved_dispatch.updated_at);
                    saved_dispatch.error_message = Some(format!(
                        "Background review task stopped unexpectedly: {join_error}"
                    ));
                    let _ = state
                        .review_dispatch_repository
                        .save_dispatch(&saved_dispatch);
                }
            }
        }
    });
}

pub fn spawn_remote_review_follow_up_reconciler(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let reconciliation_run_id =
                format!("review-follow-up-{}", now_utc().unix_timestamp_nanos());

            let reconcile_state = state.clone();
            let join_result = tokio::task::spawn_blocking(move || {
                let dispatch_service = RemoteDispatchService {
                    config_service: &reconcile_state.config_service,
                    dispatch_repository: &reconcile_state.dispatch_repository,
                    project_repository: &reconcile_state.project_repository,
                    task_repository: &reconcile_state.task_repository,
                    review_repository: &reconcile_state.review_repository,
                    review_dispatch_repository: &reconcile_state.review_dispatch_repository,
                };

                dispatch_service.reconcile_review_follow_up()
            })
            .await;

            let reconciliation = match join_result {
                Ok(Ok(reconciliation)) => reconciliation,
                Ok(Err(error)) => {
                    tracing::warn!(
                        reconciliation_run_id = %reconciliation_run_id,
                        "Review follow-up reconciliation failed: {error}"
                    );
                    continue;
                }
                Err(join_error) => {
                    tracing::warn!(
                        reconciliation_run_id = %reconciliation_run_id,
                        "Review follow-up reconciliation task failed to join: {join_error}"
                    );
                    continue;
                }
            };

            for event in &reconciliation.events {
                let branch_name = event.branch_name.as_deref().unwrap_or("");
                let pull_request_url = event.pull_request_url.as_deref().unwrap_or("");
                let pr_head_oid = event.pr_head_oid.as_deref().unwrap_or("");
                let latest_review_state = event.latest_review_state.as_deref().unwrap_or("");
                let latest_review_submitted_at =
                    event.latest_review_submitted_at.as_deref().unwrap_or("");

                let task_event = tracing::info_span!(
                    "review_follow_up_task_event",
                    reconciliation_run_id = %reconciliation_run_id,
                    outcome = %event.outcome,
                    task_id = %event.task_id,
                    dispatch_id = %event.dispatch_id,
                    dispatch_status = %event.dispatch_status,
                    remote_host = %event.remote_host,
                    branch_name = %branch_name,
                    pull_request_url = %pull_request_url,
                    reviewer = %event.reviewer,
                    pr_is_open = ?event.pr_is_open,
                    pr_head_oid = %pr_head_oid,
                    latest_review_state = %latest_review_state,
                    latest_review_submitted_at = %latest_review_submitted_at,
                );
                let _task_event_guard = task_event.enter();

                if event.outcome.ends_with("_failed") {
                    tracing::warn!("{}", event.detail);
                } else {
                    tracing::info!("{}", event.detail);
                }
            }

            if reconciliation.review_notifications_updated > 0
                || !reconciliation.queued_dispatches.is_empty()
                || reconciliation.failures > 0
            {
                tracing::info!(
                    reconciliation_run_id = %reconciliation_run_id,
                    review_notifications_updated = reconciliation.review_notifications_updated,
                    queued_dispatches = reconciliation.queued_dispatches.len(),
                    failures = reconciliation.failures,
                    evaluated_events = reconciliation.events.len(),
                    "Review follow-up reconciliation applied updates"
                );
            }

            if !reconciliation.queued_dispatches.is_empty() {
                bump_task_change_version(&state);
            }

            for queued_dispatch in reconciliation.queued_dispatches {
                spawn_dispatch_launch(state.clone(), queued_dispatch);
            }
        }
    });
}

async fn dispatch_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let input = if body.is_empty() {
        DispatchTaskInput::default()
    } else {
        serde_json::from_slice::<DispatchTaskInput>(&body)
            .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?
    };

    let queue_state = state.clone();
    let task_id = id.clone();
    let dispatch = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &queue_state.config_service,
            dispatch_repository: &queue_state.dispatch_repository,
            project_repository: &queue_state.project_repository,
            task_repository: &queue_state.task_repository,
            review_repository: &queue_state.review_repository,
            review_dispatch_repository: &queue_state.review_dispatch_repository,
        };

        dispatch_service.queue_dispatch(&task_id, input.preferred_tool)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Dispatch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

async fn follow_up_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let queue_state = state.clone();
    let task_id = id.clone();
    let dispatch = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &queue_state.config_service,
            dispatch_repository: &queue_state.dispatch_repository,
            project_repository: &queue_state.project_repository,
            task_repository: &queue_state.task_repository,
            review_repository: &queue_state.review_repository,
            review_dispatch_repository: &queue_state.review_dispatch_repository,
        };

        dispatch_service.queue_follow_up_dispatch(&task_id, &input.request)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Follow-up task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    bump_task_change_version(&state);

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

async fn cancel_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let state = state.clone();
    let canceled_dispatch = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &state.config_service,
            dispatch_repository: &state.dispatch_repository,
            project_repository: &state.project_repository,
            task_repository: &state.task_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        dispatch_service.cancel_dispatch(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Cancel dispatch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(canceled_dispatch))
}

async fn cancel_review_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let state = state.clone();
    let canceled_dispatch = tokio::task::spawn_blocking(move || {
        let review_service = RemoteReviewService {
            config_service: &state.config_service,
            project_repository: &state.project_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        review_service.cancel_dispatch(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Cancel review task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(canceled_dispatch))
}

async fn discard_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &state.config_service,
            dispatch_repository: &state.dispatch_repository,
            project_repository: &state.project_repository,
            task_repository: &state.task_repository,
            review_repository: &state.review_repository,
            review_dispatch_repository: &state.review_dispatch_repository,
        };

        dispatch_service.discard_dispatch_history(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Discard dispatch task failed to join: {error}")))?
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

async fn enforce_migration_gate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<AxumResponse, ApiError> {
    let migration = state
        .migration_service
        .status()
        .map_err(ApiError::from_track_error)?;
    if migration.requires_migration {
        return Err(ApiError::from_track_error(TrackError::new(
            ErrorCode::MigrationRequired,
            "Legacy track data must be imported before the backend can serve normal API routes.",
        )));
    }

    Ok(next.run(request).await)
}

pub fn build_app(state: AppState, static_root: impl AsRef<Path>) -> Router {
    // The deployed app still serves both API routes and the frontend from one
    // process so Docker can expose a single local port.
    let static_root = static_root.as_ref().to_path_buf();
    let migration_router = Router::new()
        .route("/meta/server_version", get(get_server_version))
        .route("/migration/status", get(migration_status))
        .route("/migration/import", post(import_legacy_data));

    // The migration release has two distinct backend modes. Migration routes
    // stay available at all times so the UI and CLI can recover gracefully,
    // while the rest of the API refuses normal work until the legacy import
    // finishes.
    let application_router = Router::new()
        .route("/projects", get(list_projects))
        .route(
            "/projects/{canonical_name}",
            put(put_project).patch(patch_project),
        )
        .route(
            "/remote-agent",
            get(get_remote_agent_settings)
                .put(put_remote_agent_settings)
                .patch(patch_remote_agent_settings),
        )
        .route(
            "/remote-agent/cleanup",
            post(cleanup_remote_agent_artifacts),
        )
        .route("/remote-agent/reset", post(reset_remote_agent_workspace))
        .route("/dispatches", get(list_dispatches))
        .route("/reviews", get(list_reviews).post(create_review))
        .route("/reviews/{id}", axum::routing::delete(delete_review))
        .route("/reviews/{id}/runs", get(list_review_runs))
        .route("/reviews/{id}/follow-up", post(follow_up_review))
        .route("/reviews/{id}/cancel", post(cancel_review_dispatch))
        .route("/runs", get(list_runs))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/{id}/runs", get(list_task_runs))
        .route("/tasks/{id}", patch(patch_task).delete(delete_task))
        .route(
            "/tasks/{id}/dispatch",
            post(dispatch_task).delete(discard_task_dispatch),
        )
        .route("/tasks/{id}/follow-up", post(follow_up_task))
        .route("/tasks/{id}/dispatch/cancel", post(cancel_task_dispatch))
        .route("/events/version", get(get_task_change_version))
        .route(
            "/events/tasks-changed",
            axum::routing::post(notify_task_change),
        )
        .fallback(api_not_found)
        .route_layer(from_fn_with_state(state.clone(), enforce_migration_gate));

    let api_router = migration_router.merge(application_router);

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
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;
    use track_core::backend_config::{BackendConfigRepository, RemoteAgentConfigService};
    use track_core::config::{
        RemoteAgentConfigFile, DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
        DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
    };
    use track_core::database::DatabaseContext;
    use track_core::dispatch_repository::DispatchRepository;
    use track_core::migration_service::MigrationService;
    use track_core::paths::{
        get_backend_managed_remote_agent_key_path,
        get_backend_managed_remote_agent_known_hosts_path,
    };
    use track_core::project_catalog::ProjectInfo;
    use track_core::project_repository::{ProjectMetadata, ProjectRepository};
    use track_core::review_dispatch_repository::ReviewDispatchRepository;
    use track_core::review_repository::ReviewRepository;
    use track_core::settings_repository::SettingsRepository;
    use track_core::task_repository::FileTaskRepository;
    use track_core::time_utils::now_utc;
    use track_core::types::{
        DispatchStatus, Priority, RemoteAgentPreferredTool, ReviewRecord, ReviewRunRecord,
        TaskCreateInput, TaskSource,
    };

    use super::{build_app, AppState};

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

    fn test_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ScopedEnvVar {
        key: &'static str,
        previous_value: Option<OsString>,
    }

    impl ScopedEnvVar {
        fn set_path(key: &'static str, value: PathBuf) -> Self {
            let previous_value = std::env::var_os(key);
            std::env::set_var(key, value);

            Self {
                key,
                previous_value,
            }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            match self.previous_value.take() {
                Some(previous_value) => std::env::set_var(self.key, previous_value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct TestEnvironment {
        _env_lock: std::sync::MutexGuard<'static, ()>,
        _track_state_dir_guard: ScopedEnvVar,
        _track_legacy_root_guard: ScopedEnvVar,
        _track_legacy_config_guard: ScopedEnvVar,
    }

    impl TestEnvironment {
        fn new(directory: &TempDir) -> Self {
            let env_lock = test_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let backend_state_dir = directory.path().join("backend");
            let legacy_root = directory.path().join("legacy-root");
            let legacy_config_path = directory.path().join("legacy-config/config.json");

            Self {
                _env_lock: env_lock,
                _track_state_dir_guard: ScopedEnvVar::set_path(
                    "TRACK_STATE_DIR",
                    backend_state_dir,
                ),
                _track_legacy_root_guard: ScopedEnvVar::set_path("TRACK_LEGACY_ROOT", legacy_root),
                _track_legacy_config_guard: ScopedEnvVar::set_path(
                    "TRACK_LEGACY_CONFIG_PATH",
                    legacy_config_path,
                ),
            }
        }
    }

    fn config_service(directory: &TempDir) -> Arc<RemoteAgentConfigService> {
        let database = DatabaseContext::new(Some(database_path(directory)))
            .expect("database context should resolve");
        let settings =
            SettingsRepository::new(Some(database)).expect("settings repository should resolve");
        let repository = BackendConfigRepository::new(Some(settings))
            .expect("backend config repository should resolve");

        Arc::new(
            RemoteAgentConfigService::new(Some(repository))
                .expect("remote-agent config service should resolve"),
        )
    }

    fn dispatch_repository(directory: &TempDir) -> Arc<DispatchRepository> {
        Arc::new(
            DispatchRepository::new(Some(database_path(directory)))
                .expect("dispatch repository should resolve"),
        )
    }

    fn project_repository(directory: &TempDir) -> Arc<ProjectRepository> {
        Arc::new(
            ProjectRepository::new(Some(database_path(directory)))
                .expect("project repository should resolve"),
        )
    }

    fn review_repository(directory: &TempDir) -> Arc<ReviewRepository> {
        Arc::new(
            ReviewRepository::new(Some(database_path(directory)))
                .expect("review repository should resolve"),
        )
    }

    fn review_dispatch_repository(directory: &TempDir) -> Arc<ReviewDispatchRepository> {
        Arc::new(
            ReviewDispatchRepository::new(Some(database_path(directory)))
                .expect("review dispatch repository should resolve"),
        )
    }

    fn task_repository(directory: &TempDir) -> Arc<FileTaskRepository> {
        Arc::new(
            FileTaskRepository::new(Some(database_path(directory)))
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

    fn register_project(project_repository: &ProjectRepository, canonical_name: &str) {
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
            .expect("project should save");
    }

    fn configured_remote_agent_config_service(
        directory: &TempDir,
    ) -> Arc<RemoteAgentConfigService> {
        let service = config_service(directory);
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
            .expect("remote-agent config should save");
        service
    }

    #[tokio::test]
    async fn lists_tasks_with_backend_sorting() {
        let directory = TempDir::new().expect("tempdir should be created");
        let _environment = TestEnvironment::new(&directory);
        let static_root = static_root(&directory);
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let repository = task_repository(&directory);
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
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let repository = task_repository(&directory);

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
            .expect("stored tasks should load");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source, Some(TaskSource::Web));
    }

    #[tokio::test]
    async fn preserves_cli_source_when_task_is_created_through_the_api() {
        let directory = TempDir::new().expect("tempdir should be created");
        let _environment = TestEnvironment::new(&directory);
        let static_root = static_root(&directory);
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let repository = task_repository(&directory);

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
            .expect("stored tasks should load");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source, Some(TaskSource::Cli));
    }

    #[tokio::test]
    async fn lists_dispatches_for_single_and_repeated_task_ids() {
        let directory = TempDir::new().expect("tempdir should be created");
        let _environment = TestEnvironment::new(&directory);
        let static_root = static_root(&directory);
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        register_project(&project_repository, "project-b");
        let task_repository = task_repository(&directory);
        let dispatch_repository = dispatch_repository(&directory);

        let first_task = task_repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "First dispatched task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("first task should be created")
            .task;
        let second_task = task_repository
            .create_task(TaskCreateInput {
                project: "project-b".to_owned(),
                priority: Priority::Medium,
                description: "Second dispatched task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("second task should be created")
            .task;

        dispatch_repository
            .create_dispatch(&first_task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("first dispatch should be created");
        dispatch_repository
            .create_dispatch(&second_task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("second dispatch should be created");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository,
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let repeated_json: serde_json::Value = serde_json::from_slice(&repeated_body)
            .expect("repeated-dispatch response should be json");
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let task_repository = task_repository(&directory);
        let dispatch_repository = dispatch_repository(&directory);

        let task = task_repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "Investigate an agent run".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created")
            .task;
        let dispatch = dispatch_repository
            .create_dispatch(&task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("dispatch should be created");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository,
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let task_repository = task_repository(&directory);
        let dispatch_repository = dispatch_repository(&directory);

        let task = task_repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "Inspect task-scoped run history".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created")
            .task;

        dispatch_repository
            .create_dispatch(&task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("first dispatch should be created");
        dispatch_repository
            .create_dispatch(&task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("second dispatch should be created");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository,
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let review_repository = review_repository(&directory);
        let review_dispatch_repository = review_dispatch_repository(&directory);
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
            .expect("review run should save");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository(&directory),
                review_dispatch_repository,
                review_repository,
                task_repository(&directory),
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let task_repository = task_repository(&directory);
        let dispatch_repository = dispatch_repository(&directory);

        let task = task_repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "Discardable dispatch".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created")
            .task;

        let mut dispatch = dispatch_repository
            .create_dispatch(&task, "192.0.2.25", RemoteAgentPreferredTool::Codex)
            .expect("dispatch should be created");
        dispatch.status = DispatchStatus::Failed;
        dispatch.finished_at = Some(dispatch.updated_at);
        dispatch_repository
            .save_dispatch(&dispatch)
            .expect("terminal dispatch should save");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository.clone(),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let repository = task_repository(&directory);
        let created = repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Update the onboarding guide".to_owned(),
                source: Some(TaskSource::Web),
            })
            .expect("task should be created");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let project_repository = project_repository(&directory);
        register_project(&project_repository, "project-a");
        let repository = task_repository(&directory);
        let created = repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Versioned task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let project_repository = project_repository(&directory);
        project_repository
            .ensure_project(&ProjectInfo {
                canonical_name: "project-a".to_owned(),
                path: project_path,
                aliases: vec![],
            })
            .expect("project should initialize");

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
                task_repository(&directory),
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
        let project_repository = project_repository(&directory);
        let task_repository = task_repository(&directory);

        let app = build_app(
            app_state(
                config_service(&directory),
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
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
        let config_service = configured_remote_agent_config_service(&directory);
        let project_repository = project_repository(&directory);

        let app = build_app(
            app_state(
                config_service,
                dispatch_repository(&directory),
                project_repository,
                review_dispatch_repository(&directory),
                review_repository(&directory),
                task_repository(&directory),
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
        let config_service = config_service(&directory);

        let app = build_app(
            app_state(
                config_service,
                dispatch_repository(&directory),
                project_repository(&directory),
                review_dispatch_repository(&directory),
                review_repository(&directory),
                task_repository(&directory),
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
}
