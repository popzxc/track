use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_types::ids::ReviewId;
use track_types::time_utils::now_utc;
use track_types::types::{CreateReviewInput, ReviewRecord, ReviewRunRecord};

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewSummaryResponse {
    review: ReviewRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_run: Option<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewsResponse {
    reviews: Vec<ReviewSummaryResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRunsResponse {
    runs: Vec<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateReviewResponse {
    review: ReviewRecord,
    run: ReviewRunRecord,
}

pub(crate) async fn list_reviews(
    State(state): State<AppState>,
) -> Result<Json<ReviewsResponse>, ApiError> {
    let reviews = state
        .database
        .review_repository()
        .list_reviews()
        .await
        .map_err(ApiError::from_track_error)?;
    let review_ids = reviews
        .iter()
        .map(|review| review.id.clone())
        .collect::<Vec<_>>();
    let latest_runs = state
        .database
        .review_dispatch_repository()
        .latest_dispatches_for_reviews(&review_ids)
        .await
        .map_err(ApiError::from_track_error)?;
    let latest_runs = state
        .refresh_review_run_records_if_active(latest_runs)
        .await
        .map_err(ApiError::from_track_error)?;
    let latest_runs_by_review_id = latest_runs
        .into_iter()
        .map(|run| (run.review_id.clone(), run))
        .collect::<std::collections::BTreeMap<_, _>>();
    let reviews = reviews
        .into_iter()
        .map(|review| ReviewSummaryResponse {
            latest_run: latest_runs_by_review_id.get(&review.id).cloned(),
            review,
        })
        .collect::<Vec<_>>();
    tracing::info!(review_count = reviews.len(), "Listed reviews");

    Ok(Json(ReviewsResponse { reviews }))
}

#[tracing::instrument(skip(state), fields(review_id = %id))]
pub(crate) async fn list_review_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<ReviewRunsResponse>, ApiError> {
    let runs = state
        .database
        .review_dispatch_repository()
        .dispatches_for_review(&id)
        .await
        .map_err(ApiError::from_track_error)?;
    let runs = state
        .refresh_review_run_records_if_active(runs)
        .await
        .map_err(ApiError::from_track_error)?;
    tracing::info!(run_count = runs.len(), "Listed review run history");

    Ok(Json(ReviewRunsResponse { runs }))
}

#[tracing::instrument(skip(state, body))]
pub(crate) async fn create_review(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<CreateReviewResponse>, ApiError> {
    let input = serde_json::from_slice::<CreateReviewInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let (review, run) = {
        let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
        state
            .remote_agent_runtime_services()
            .await
            .map_err(ApiError::from_track_error)?
            .review()
            .create_review(input)
            .await
            .map_err(ApiError::from_track_error)?
    };
    crate::app::bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());
    tracing::info!(
        review_id = %review.id,
        dispatch_id = %run.run.dispatch_id,
        remote_host = %run.run.remote_host,
        preferred_tool = ?run.run.preferred_tool,
        "Created review from API"
    );

    Ok(Json(CreateReviewResponse { review, run }))
}

// TODO: duplicated
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FollowUpRequestInput {
    request: String,
}

#[tracing::instrument(skip(state, body), fields(review_id = %id))]
pub(crate) async fn follow_up_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
    body: Bytes,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let run = {
        let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
        state
            .remote_agent_runtime_services()
            .await
            .map_err(ApiError::from_track_error)?
            .review()
            .queue_follow_up_review_dispatch(&id, &input.request)
            .await
            .map_err(ApiError::from_track_error)?
    };
    crate::app::bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());
    tracing::info!(
        dispatch_id = %run.run.dispatch_id,
        remote_host = %run.run.remote_host,
        "Queued review follow-up from API"
    );

    Ok(Json(run))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteReviewResponse {
    ok: bool,
}

#[tracing::instrument(skip(state), fields(review_id = %id))]
pub(crate) async fn delete_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<DeleteReviewResponse>, ApiError> {
    let has_dispatch_history = !state
        .database
        .review_dispatch_repository()
        .dispatches_for_review(&id)
        .await
        .map_err(ApiError::from_track_error)?
        .is_empty();

    if has_dispatch_history {
        let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
        state
            .remote_agent_runtime_services()
            .await
            .map_err(ApiError::from_track_error)?
            .review()
            .delete_review(&id)
            .await
            .map_err(ApiError::from_track_error)?;
    } else {
        state
            .database
            .review_repository()
            .delete_review(&id)
            .await
            .map_err(ApiError::from_track_error)?;
    }
    crate::app::bump_task_change_version(&state);
    tracing::info!("Deleted review");

    Ok(Json(DeleteReviewResponse { ok: true }))
}

#[tracing::instrument(skip(state), fields(review_id = %id))]
pub(crate) async fn cancel_review_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let canceled_dispatch = {
        let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
        state
            .remote_agent_runtime_services()
            .await
            .map_err(ApiError::from_track_error)?
            .review()
            .cancel_dispatch(&id)
            .await
            .map_err(ApiError::from_track_error)?
    };
    tracing::info!(
        dispatch_id = %canceled_dispatch.run.dispatch_id,
        "Canceled review dispatch from API"
    );

    Ok(Json(canceled_dispatch))
}

pub(crate) fn spawn_review_launch(state: AppState, queued_dispatch: ReviewRunRecord) {
    tokio::spawn(async move {
        tracing::info!(
            review_id = %queued_dispatch.review_id,
            dispatch_id = %queued_dispatch.run.dispatch_id,
            remote_host = %queued_dispatch.run.remote_host,
            "Starting background review launch"
        );
        let launch_result = async {
            let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
            state
                .remote_agent_runtime_services()
                .await?
                .review()
                .launch_prepared_review(queued_dispatch.clone())
                .await
        }
        .await;

        if let Err(join_error) = launch_result {
            tracing::error!(
                review_id = %queued_dispatch.review_id,
                dispatch_id = %queued_dispatch.run.dispatch_id,
                "Background review launch failed"
            );
            if let Some(mut saved_dispatch) = state
                .database
                .review_dispatch_repository()
                .get_dispatch(&queued_dispatch.review_id, &queued_dispatch.run.dispatch_id)
                .await
                .ok()
                .flatten()
            {
                if saved_dispatch.run.status.is_active() {
                    saved_dispatch.run.status = track_types::types::DispatchStatus::Failed;
                    saved_dispatch.run.updated_at = now_utc();
                    saved_dispatch.run.finished_at = Some(saved_dispatch.run.updated_at);
                    saved_dispatch.run.error_message = Some(format!(
                        "Background review task stopped unexpectedly: {join_error}"
                    ));
                    let _ = state
                        .database
                        .review_dispatch_repository()
                        .save_dispatch(&saved_dispatch)
                        .await;
                }
            }
        } else {
            tracing::info!(
                review_id = %queued_dispatch.review_id,
                dispatch_id = %queued_dispatch.run.dispatch_id,
                "Background review launch finished"
            );
        }
    });
}
