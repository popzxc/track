use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_types::types::{Task, TaskDispatchRecord};

use crate::api_error::ApiError;
use crate::AppState;

// TODO: Used elsewhere but shouldn't (probably)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunRecordResponse {
    pub(crate) task: Task,
    pub(crate) dispatch: TaskDispatchRecord,
}

// TODO: Used elsewhere but shouldn't (probably)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunsResponse {
    pub(crate) runs: Vec<RunRecordResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunsQuery {
    limit: Option<usize>,
}

pub(crate) async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<RunsResponse>, ApiError> {
    let limit = query.limit;
    let dispatches = state
        .remote_run_queries()
        .global_task_dispatches(limit)
        .await
        .map_err(ApiError::from_track_error)?;
    let tasks_by_id = state
        .database
        .task_repository()
        .tasks_by_ids(
            &dispatches
                .iter()
                .map(|dispatch| dispatch.task_id.clone())
                .collect::<Vec<_>>(),
        )
        .await
        .map_err(ApiError::from_track_error)?
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut runs = Vec::new();
    for dispatch in dispatches {
        let Some(task) = tasks_by_id.get(&dispatch.task_id).cloned() else {
            // Runs are persisted separately from task files. If a task was
            // deleted later, we prefer to hide that orphaned run from the
            // normal UI instead of turning the whole page into an error.
            continue;
        };
        runs.push(RunRecordResponse { task, dispatch });
    }
    tracing::info!(limit = ?limit, run_count = runs.len(), "Listed runs");

    Ok(Json(RunsResponse { runs }))
}
