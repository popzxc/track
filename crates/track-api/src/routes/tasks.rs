use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_types::ids::{ProjectId, TaskId};
use track_types::task_sort::sort_tasks;
use track_types::time_utils::now_utc;
use track_types::types::{
    RemoteAgentPreferredTool, Task, TaskCreateInput, TaskDispatchRecord, TaskSource,
    TaskUpdateInput,
};

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskListQuery {
    include_closed: Option<bool>,
    project: Option<ProjectId>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TasksResponse {
    tasks: Vec<Task>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteTaskResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FollowUpRequestInput {
    request: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchTaskInput {
    #[serde(default)]
    preferred_tool: Option<RemoteAgentPreferredTool>,
}

pub(crate) async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<TaskListQuery>,
) -> Result<Json<TasksResponse>, ApiError> {
    let tasks = state
        .database
        .task_repository()
        .list_tasks(
            query.include_closed.unwrap_or(false),
            query.project.as_ref(),
        )
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(TasksResponse {
        tasks: sort_tasks(&tasks),
    }))
}

pub(crate) async fn list_task_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
) -> Result<Json<super::runs::RunsResponse>, ApiError> {
    let task = state
        .database
        .task_repository()
        .get_task(&id)
        .await
        .map_err(ApiError::from_track_error)?;
    let dispatches = state
        .remote_agent_services()
        .dispatch()
        .dispatch_history_for_task(&id)
        .await
        .map_err(ApiError::from_track_error)?;
    let runs = dispatches
        .into_iter()
        .map(|dispatch| super::runs::RunRecordResponse {
            task: task.clone(),
            dispatch,
        })
        .collect::<Vec<_>>();

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
        .database
        .task_repository()
        .create_task(validated_input)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(created_task.task))
}

pub(crate) async fn patch_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
    body: Bytes,
) -> Result<Json<Task>, ApiError> {
    let input = serde_json::from_slice::<TaskUpdateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let validated_input = input.validate().map_err(ApiError::from_track_error)?;

    let updated_task = state
        .remote_agent_services()
        .dispatch()
        .update_task(&id, validated_input)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(updated_task))
}

pub(crate) async fn delete_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    state
        .remote_agent_services()
        .dispatch()
        .delete_task(&id)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(DeleteTaskResponse { ok: true }))
}

pub(crate) async fn dispatch_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
    body: Bytes,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let input = if body.is_empty() {
        DispatchTaskInput::default()
    } else {
        serde_json::from_slice::<DispatchTaskInput>(&body)
            .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?
    };

    let dispatch = state
        .remote_agent_services()
        .dispatch()
        .queue_dispatch(&id, input.preferred_tool)
        .await
        .map_err(ApiError::from_track_error)?;

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

pub(crate) async fn follow_up_task(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
    body: Bytes,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let dispatch = state
        .remote_agent_services()
        .dispatch()
        .queue_follow_up_dispatch(&id, &input.request)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    spawn_dispatch_launch(state.clone(), dispatch.clone());

    Ok(Json(dispatch))
}

pub(crate) async fn cancel_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
) -> Result<Json<TaskDispatchRecord>, ApiError> {
    let canceled_dispatch = state
        .remote_agent_services()
        .dispatch()
        .cancel_dispatch(&id)
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(canceled_dispatch))
}

pub(crate) async fn discard_task_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<TaskId>,
) -> Result<Json<DeleteTaskResponse>, ApiError> {
    state
        .remote_agent_services()
        .dispatch()
        .discard_dispatch_history(&id)
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(DeleteTaskResponse { ok: true }))
}

// TODO: Used elsewhere -- is this a right location?
pub(crate) fn spawn_dispatch_launch(state: AppState, queued_dispatch: TaskDispatchRecord) {
    tokio::spawn(async move {
        let launch_result = state
            .remote_agent_services()
            .dispatch()
            .launch_prepared_dispatch(queued_dispatch.clone())
            .await;

        if let Err(join_error) = launch_result {
            if let Some(mut saved_dispatch) = state
                .database
                .dispatch_repository()
                .get_dispatch(&queued_dispatch.task_id, &queued_dispatch.dispatch_id)
                .await
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
                    let _ = state
                        .database
                        .dispatch_repository()
                        .save_dispatch(&saved_dispatch)
                        .await;
                }
            }
        }
    });
}
