use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::services::{ServeDir, ServeFile};
use track_core::config::{
    ConfigService, RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile,
};
use track_core::dispatch_repository::DispatchRepository;
use track_core::errors::{ErrorCode, TrackError};
use track_core::project_repository::{
    ProjectMetadataUpdateInput, ProjectRecord, ProjectRepository,
};
use track_core::remote_agent::RemoteDispatchService;
use track_core::task_repository::FileTaskRepository;
use track_core::task_sort::sort_tasks;
use track_core::time_utils::now_utc;
use track_core::types::{
    RemoteCleanupSummary, RemoteResetSummary, Task, TaskCreateInput, TaskDispatchRecord,
    TaskSource, TaskUpdateInput,
};

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<ConfigService>,
    pub dispatch_repository: Arc<DispatchRepository>,
    pub project_repository: Arc<ProjectRepository>,
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
struct RemoteAgentSettingsResponse {
    configured: bool,
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
    #[serde(rename = "shellPrelude")]
    shell_prelude: String,
    #[serde(rename = "reviewFollowUp")]
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
}

#[derive(Debug, Deserialize)]
struct FollowUpRequestInput {
    request: String,
}

fn remote_agent_settings_response(
    remote_agent: Option<RemoteAgentConfigFile>,
) -> RemoteAgentSettingsResponse {
    match remote_agent {
        Some(remote_agent) => RemoteAgentSettingsResponse {
            configured: true,
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
                        },
                    )
                    .unwrap_or(RemoteAgentReviewFollowUpSettingsResponse {
                        enabled: false,
                        main_user: None,
                    }),
            ),
        },
        None => RemoteAgentSettingsResponse {
            configured: false,
            host: None,
            user: None,
            port: None,
            shell_prelude: None,
            review_follow_up: None,
        },
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<ProjectsResponse>, ApiError> {
    let projects = state
        .project_repository
        .list_projects()
        .map_err(ApiError::from_track_error)?;

    Ok(Json(ProjectsResponse { projects }))
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
            "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
        )))?;

    let remote_agent = state
        .config_service
        .save_remote_agent_settings(
            Some(input.shell_prelude),
            input
                .review_follow_up
                .map(|review_follow_up| RemoteAgentReviewFollowUpConfigFile {
                    enabled: review_follow_up.enabled,
                    main_user: review_follow_up.main_user,
                })
                .or(existing_remote_agent.review_follow_up),
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
    let validated_input = TaskCreateInput {
        source: Some(TaskSource::Web),
        ..input
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
        };

        dispatch_service.delete_task(&task_id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Delete task failed to join: {error}")))?
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
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let queue_state = state.clone();
    let task_id = id.clone();
    let dispatch = tokio::task::spawn_blocking(move || {
        let dispatch_service = RemoteDispatchService {
            config_service: &queue_state.config_service,
            dispatch_repository: &queue_state.dispatch_repository,
            project_repository: &queue_state.project_repository,
            task_repository: &queue_state.task_repository,
        };

        dispatch_service.queue_dispatch(&task_id)
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
        };

        dispatch_service.cancel_dispatch(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Cancel dispatch task failed to join: {error}")))?
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

pub fn build_app(state: AppState, static_root: impl AsRef<Path>) -> Router {
    // The deployed app still serves both API routes and the frontend from one
    // process so Docker can expose a single local port.
    let static_root = static_root.as_ref().to_path_buf();
    let api_router = Router::new()
        .route("/projects", get(list_projects))
        .route("/projects/{canonical_name}", patch(patch_project))
        .route(
            "/remote-agent",
            get(get_remote_agent_settings).patch(patch_remote_agent_settings),
        )
        .route(
            "/remote-agent/cleanup",
            post(cleanup_remote_agent_artifacts),
        )
        .route("/remote-agent/reset", post(reset_remote_agent_workspace))
        .route("/dispatches", get(list_dispatches))
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
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;
    use track_core::config::{
        ApiConfigFile, ConfigService, LlamaCppConfigFile, RemoteAgentConfigFile, TrackConfigFile,
        DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
        DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
    };
    use track_core::dispatch_repository::DispatchRepository;
    use track_core::project_catalog::ProjectInfo;
    use track_core::project_repository::ProjectRepository;
    use track_core::task_repository::FileTaskRepository;
    use track_core::types::{DispatchStatus, Priority, TaskCreateInput, TaskSource};

    use super::{build_app, AppState};

    fn static_root(directory: &TempDir) -> std::path::PathBuf {
        let root = directory.path().join("static");
        fs::create_dir_all(&root).expect("static root should exist");
        fs::write(root.join("index.html"), "<html><body>track</body></html>")
            .expect("static index should be written");
        root
    }

    fn config_service(directory: &TempDir) -> Arc<ConfigService> {
        Arc::new(
            ConfigService::new(Some(directory.path().join("config.json")))
                .expect("config service should resolve"),
        )
    }

    fn configured_remote_agent_config_service(directory: &TempDir) -> Arc<ConfigService> {
        let service = config_service(directory);
        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec![],
                project_aliases: Default::default(),
                api: ApiConfigFile::default(),
                llama_cpp: LlamaCppConfigFile::default(),
                remote_agent: Some(RemoteAgentConfigFile {
                    host: "192.0.2.25".to_owned(),
                    user: "builder".to_owned(),
                    port: DEFAULT_REMOTE_AGENT_PORT,
                    workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
                    projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
                    shell_prelude: Some(". \"$HOME/.cargo/env\"".to_owned()),
                    review_follow_up: None,
                }),
            })
            .expect("remote-agent config should save");
        service
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
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository: repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
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
    async fn creates_tasks_from_the_web_api() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("repository should resolve"),
        );

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository: repository.clone(),
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
    async fn lists_dispatches_for_single_and_repeated_task_ids() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let task_repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("task repository should resolve"),
        );
        let dispatch_repository = Arc::new(
            DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                .expect("dispatch repository should resolve"),
        );

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
            .create_dispatch(&first_task, "192.0.2.25")
            .expect("first dispatch should be created");
        dispatch_repository
            .create_dispatch(&second_task, "192.0.2.25")
            .expect("second dispatch should be created");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository,
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
        let static_root = static_root(&directory);
        let task_repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("task repository should resolve"),
        );
        let dispatch_repository = Arc::new(
            DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                .expect("dispatch repository should resolve"),
        );

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
            .create_dispatch(&task, "192.0.2.25")
            .expect("dispatch should be created");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository,
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
        let static_root = static_root(&directory);
        let task_repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("task repository should resolve"),
        );
        let dispatch_repository = Arc::new(
            DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                .expect("dispatch repository should resolve"),
        );

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
            .create_dispatch(&task, "192.0.2.25")
            .expect("first dispatch should be created");
        dispatch_repository
            .create_dispatch(&task, "192.0.2.25")
            .expect("second dispatch should be created");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository,
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
    async fn discards_dispatch_history_for_a_task() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let task_repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("task repository should resolve"),
        );
        let dispatch_repository = Arc::new(
            DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                .expect("dispatch repository should resolve"),
        );

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
            .create_dispatch(&task, "192.0.2.25")
            .expect("dispatch should be created");
        dispatch.status = DispatchStatus::Failed;
        dispatch.finished_at = Some(dispatch.updated_at);
        dispatch_repository
            .save_dispatch(&dispatch)
            .expect("terminal dispatch should save");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository: dispatch_repository.clone(),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
        let static_root = static_root(&directory);

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
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository: repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
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

    #[tokio::test]
    async fn bumps_task_change_version_for_notify_and_mutations() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("repository should resolve"),
        );
        let created = repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Versioned task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository: repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
        let static_root = static_root(&directory);
        let project_path = directory.path().join("workspace/project-a");
        fs::create_dir_all(project_path.join(".git")).expect("git directory should exist");
        fs::write(
            project_path.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:acme/project-a.git\n",
        )
        .expect("git config should be written");
        let project_repository = Arc::new(
            ProjectRepository::new(Some(directory.path().join("issues")))
                .expect("project repository should resolve"),
        );
        project_repository
            .ensure_project(&ProjectInfo {
                canonical_name: "project-a".to_owned(),
                path: project_path,
                aliases: vec![],
            })
            .expect("project should initialize");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository,
                task_repository: Arc::new(
                    FileTaskRepository::new(Some(directory.path().join("issues")))
                        .expect("task repository should resolve"),
                ),
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
    async fn lists_persisted_projects_even_without_project_metadata_file() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let task_repository = Arc::new(
            FileTaskRepository::new(Some(directory.path().join("issues")))
                .expect("task repository should resolve"),
        );
        task_repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::Medium,
                description: "Project exists because a task exists".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let app = build_app(
            AppState {
                config_service: config_service(&directory),
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository,
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
            &static_root,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
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
        assert_eq!(json["projects"][0]["canonicalName"], "project-a");
        assert_eq!(json["projects"][0]["metadata"]["repoUrl"], "");
        assert_eq!(json["projects"][0]["metadata"]["baseBranch"], "main");
    }

    #[tokio::test]
    async fn gets_and_updates_remote_agent_shell_prelude() {
        let directory = TempDir::new().expect("tempdir should be created");
        let static_root = static_root(&directory);
        let config_service = configured_remote_agent_config_service(&directory);

        let app = build_app(
            AppState {
                config_service,
                dispatch_repository: Arc::new(
                    DispatchRepository::new(Some(directory.path().join("issues/.dispatches")))
                        .expect("dispatch repository should resolve"),
                ),
                project_repository: Arc::new(
                    ProjectRepository::new(Some(directory.path().join("issues")))
                        .expect("project repository should resolve"),
                ),
                task_repository: Arc::new(
                    FileTaskRepository::new(Some(directory.path().join("issues")))
                        .expect("task repository should resolve"),
                ),
                task_change_version: Arc::new(AtomicU64::new(0)),
            },
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
        assert_eq!(get_json["shellPrelude"], ". \"$HOME/.cargo/env\"");
        assert_eq!(get_json["reviewFollowUp"]["enabled"], false);

        let patch_response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/remote-agent")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"shellPrelude":"export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"","reviewFollowUp":{"enabled":true,"mainUser":"octocat"}}"#,
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
        assert_eq!(
            patch_json["shellPrelude"],
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\""
        );
        assert_eq!(patch_json["reviewFollowUp"]["enabled"], true);
        assert_eq!(patch_json["reviewFollowUp"]["mainUser"], "octocat");
    }
}
