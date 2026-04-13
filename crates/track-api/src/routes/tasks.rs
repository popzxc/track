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
    tracing::info!(
        include_closed = query.include_closed.unwrap_or(false),
        project = ?query.project,
        task_count = tasks.len(),
        "Listed tasks"
    );

    Ok(Json(TasksResponse {
        tasks: sort_tasks(&tasks),
    }))
}

#[tracing::instrument(skip(state), fields(task_id = %id))]
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
    tracing::info!(run_count = runs.len(), "Listed task run history");

    Ok(Json(super::runs::RunsResponse { runs }))
}

#[tracing::instrument(skip(state, body))]
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
    tracing::info!(
        task_id = %created_task.task.id,
        project = %created_task.task.project,
        source = ?created_task.task.source,
        "Created task"
    );

    Ok(Json(created_task.task))
}

#[tracing::instrument(skip(state, body), fields(task_id = %id))]
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
    tracing::info!(status = ?updated_task.status, project = %updated_task.project, "Patched task");

    Ok(Json(updated_task))
}

#[tracing::instrument(skip(state), fields(task_id = %id))]
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
    tracing::info!("Deleted task");

    Ok(Json(DeleteTaskResponse { ok: true }))
}

#[tracing::instrument(skip(state, body), fields(task_id = %id))]
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
    tracing::info!(
        dispatch_id = %dispatch.dispatch_id,
        remote_host = %dispatch.remote_host,
        preferred_tool = ?dispatch.preferred_tool,
        "Queued task dispatch from API"
    );

    Ok(Json(dispatch))
}

#[tracing::instrument(skip(state, body), fields(task_id = %id))]
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
    tracing::info!(
        dispatch_id = %dispatch.dispatch_id,
        remote_host = %dispatch.remote_host,
        "Queued task follow-up dispatch from API"
    );

    Ok(Json(dispatch))
}

#[tracing::instrument(skip(state), fields(task_id = %id))]
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
    tracing::info!(
        dispatch_id = %canceled_dispatch.dispatch_id,
        "Canceled task dispatch from API"
    );

    Ok(Json(canceled_dispatch))
}

#[tracing::instrument(skip(state), fields(task_id = %id))]
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
    tracing::info!("Discarded task dispatch history from API");

    Ok(Json(DeleteTaskResponse { ok: true }))
}

// TODO: Used elsewhere -- is this a right location?
pub(crate) fn spawn_dispatch_launch(state: AppState, queued_dispatch: TaskDispatchRecord) {
    tokio::spawn(async move {
        tracing::info!(
            task_id = %queued_dispatch.task_id,
            dispatch_id = %queued_dispatch.dispatch_id,
            remote_host = %queued_dispatch.remote_host,
            "Starting background task dispatch launch"
        );
        let launch_result = state
            .remote_agent_services()
            .dispatch()
            .launch_prepared_dispatch(queued_dispatch.clone())
            .await;

        if let Err(join_error) = launch_result {
            tracing::error!(
                task_id = %queued_dispatch.task_id,
                dispatch_id = %queued_dispatch.dispatch_id,
                "Background task dispatch launch failed"
            );
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
        } else {
            tracing::info!(
                task_id = %queued_dispatch.task_id,
                dispatch_id = %queued_dispatch.dispatch_id,
                "Background task dispatch launch finished"
            );
        }
    });
}
