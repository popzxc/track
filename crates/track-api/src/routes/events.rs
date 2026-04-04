use std::sync::atomic::Ordering;

use axum::{extract::State, Json};
use serde::Serialize;

use crate::AppState;

#[derive(Debug, Serialize)]
pub(crate) struct TaskChangeVersionResponse {
    version: u64,
}

pub(crate) async fn get_task_change_version(
    State(state): State<AppState>,
) -> Json<TaskChangeVersionResponse> {
    Json(TaskChangeVersionResponse {
        version: state.task_change_version.load(Ordering::SeqCst),
    })
}

pub(crate) async fn notify_task_change(
    State(state): State<AppState>,
) -> Json<TaskChangeVersionResponse> {
    Json(TaskChangeVersionResponse {
        version: crate::app::bump_task_change_version(&state),
    })
}
