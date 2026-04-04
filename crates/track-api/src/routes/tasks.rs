use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_types::errors::TrackError;
use track_types::task_sort::sort_tasks;
use track_types::time_utils::now_utc;
use track_types::types::{
    RemoteAgentPreferredTool, Task, TaskCreateInput, TaskDispatchRecord, TaskSource,
    TaskUpdateInput,
};

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct TaskListQuery {
    #[serde(rename = "includeClosed")]
    include_closed: Option<bool>,
    project: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TasksResponse {
    tasks: Vec<Task>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeleteTaskResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FollowUpRequestInput {
    request: String,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DispatchTaskInput {
    #[serde(rename = "preferredTool", default)]
    preferred_tool: Option<RemoteAgentPreferredTool>,
}

pub(crate) async fn list_tasks(
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

pub(crate) async fn list_task_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<super::runs::RunsResponse>, ApiError> {
    let state = state.clone();
    let task_id = id.clone();
    let runs = tokio::task::spawn_blocking(move || {
        let task = state.task_repository.get_task(&task_id)?;
        let dispatches = state
            .remote_agent_services()
            .dispatch()
            .dispatch_history_for_task(&task_id)?;

        Ok::<Vec<super::runs::RunRecordResponse>, TrackError>(
            dispatches
                .into_iter()
                .map(|dispatch| super::runs::RunRecordResponse {
                    task: task.clone(),
                    dispatch,
                })
                .collect(),
        )
    })
    .await
    .map_err(|error| ApiError::internal(format!("Task runs refresh failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(super::runs::RunsResponse { runs }))
}

pub(crate) async fn create_task(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<Task>, ApiError> {
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
    crate::app::bump_task_change_version(&state);

    Ok(Json(created_task.task))
}

pub(crate) async fn patch_task(
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
        patch_state
            .remote_agent_services()
            .dispatch()
            .update_task(&task_id, validated_input)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Patch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(updated_task))
}

pub(crate) async fn delete_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    let delete_state = state.clone();
    let task_id = id.clone();
    tokio::task::spawn_blocking(move || {
        delete_state
            .remote_agent_services()
            .dispatch()
            .delete_task(&task_id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Delete task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(DeleteTaskResponse { ok: true }))
}

pub(crate) async fn dispatch_task(
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
        queue_state
            .remote_agent_services()
            .dispatch()
            .queue_dispatch(&task_id, input.preferred_tool)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Dispatch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

pub(crate) async fn follow_up_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let queue_state = state.clone();
    let task_id = id.clone();
    let dispatch = tokio::task::spawn_blocking(move || {
        queue_state
            .remote_agent_services()
            .dispatch()
            .queue_follow_up_dispatch(&task_id, &input.request)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Follow-up task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

pub(crate) async fn cancel_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let state = state.clone();
    let canceled_dispatch = tokio::task::spawn_blocking(move || {
        state
            .remote_agent_services()
            .dispatch()
            .cancel_dispatch(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Cancel dispatch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(canceled_dispatch))
}

pub(crate) async fn discard_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        state
            .remote_agent_services()
            .dispatch()
            .discard_dispatch_history(&id)
    })
    .await
    .map_err(|error| ApiError::internal(format!("Discard dispatch task failed to join: {error}")))?
    .map_err(ApiError::from_track_error)?;

    Ok(Json(DeleteTaskResponse { ok: true }))
}

// TODO: Used elsewhere -- is this a right location?
pub(crate) fn spawn_dispatch_launch(state: AppState, queued_dispatch: TaskDispatchRecord) {
    tokio::spawn(async move {
        let launch_state = state.clone();
        let launch_dispatch = queued_dispatch.clone();
        let join_result = tokio::task::spawn_blocking(move || {
            launch_state
                .remote_agent_services()
                .dispatch()
                .launch_prepared_dispatch(launch_dispatch)
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
                    saved_dispatch.status = track_types::types::DispatchStatus::Failed;
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
