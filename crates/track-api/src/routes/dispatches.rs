use axum::extract::State;
use axum::http::Uri;
use axum::Json;
use serde::Serialize;
use track_types::ids::TaskId;
use track_types::types::TaskDispatchRecord;

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchesResponse {
    dispatches: Vec<TaskDispatchRecord>,
}

#[tracing::instrument(skip(state, uri), fields(raw_query = ?uri.query()))]
pub(crate) async fn list_dispatches(
    State(state): State<AppState>,
    uri: Uri,
) -> Result<Json<DispatchesResponse>, ApiError> {
    let task_ids = parse_dispatch_task_ids(uri.query());
    let dispatches = state
        .remote_run_queries()
        .latest_task_dispatches(&task_ids)
        .await
        .map_err(ApiError::from_track_error)?;
    tracing::info!(
        task_count = task_ids.len(),
        dispatch_count = dispatches.len(),
        "Listed latest dispatches"
    );

    Ok(Json(DispatchesResponse { dispatches }))
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
fn parse_dispatch_task_ids(raw_query: Option<&str>) -> Vec<TaskId> {
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

            TaskId::new(value).ok()
        })
        .collect()
}
