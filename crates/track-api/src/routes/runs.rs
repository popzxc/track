use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_types::errors::ErrorCode;
use track_types::types::{Task, TaskDispatchRecord};

use crate::api_error::ApiError;
use crate::AppState;

// TODO: Used elsewhere but shouldn't (probably)
#[derive(Debug, Serialize)]
pub(crate) struct RunRecordResponse {
    pub(crate) task: Task,
    pub(crate) dispatch: TaskDispatchRecord,
}

// TODO: Used elsewhere but shouldn't (probably)
#[derive(Debug, Serialize)]
pub(crate) struct RunsResponse {
    pub(crate) runs: Vec<RunRecordResponse>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RunsQuery {
    limit: Option<usize>,
}

pub(crate) async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let limit = query.limit;
    let dispatches = state
        .remote_agent_services()
        .dispatch()
        .list_dispatches(limit)
        .await
        .map_err(ApiError::from_track_error)?;
    let mut runs = Vec::new();
    for dispatch in dispatches {
        let task = match state.task_repository.get_task(&dispatch.task_id).await {
            Ok(task) => task,
            // Runs are persisted separately from task files. If a task was
            // deleted later, we prefer to hide that orphaned run from the
            // normal UI instead of turning the whole page into an error.
            Err(error) if error.code == ErrorCode::TaskNotFound => continue,
            Err(error) => return Err(ApiError::from_track_error(error)),
        };
        runs.push(RunRecordResponse { task, dispatch });
    }

    Ok(Json(RunsResponse { runs }))
}
