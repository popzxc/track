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
pub(crate) struct ReviewSummaryResponse {
    review: ReviewRecord,
    #[serde(rename = "latestRun", skip_serializing_if = "Option::is_none")]
    latest_run: Option<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReviewsResponse {
    reviews: Vec<ReviewSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReviewRunsResponse {
    runs: Vec<ReviewRunRecord>,
}

#[derive(Debug, Serialize)]
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
        .remote_agent_services()
        .review()
        .latest_dispatches_for_reviews(&review_ids)
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

    Ok(Json(ReviewsResponse { reviews }))
}

pub(crate) async fn list_review_runs(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<ReviewRunsResponse>, ApiError> {
    let runs = state
        .remote_agent_services()
        .review()
        .dispatch_history_for_review(&id)
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(ReviewRunsResponse { runs }))
}

pub(crate) async fn create_review(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<CreateReviewResponse>, ApiError> {
    let input = serde_json::from_slice::<CreateReviewInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let (review, run) = state
        .remote_agent_services()
        .review()
        .create_review(input)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());

    Ok(Json(CreateReviewResponse { review, run }))
}

// TODO: duplicated
#[derive(Debug, Deserialize)]
pub(crate) struct FollowUpRequestInput {
    request: String,
}

pub(crate) async fn follow_up_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
    body: Bytes,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let input = serde_json::from_slice::<FollowUpRequestInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let run = state
        .remote_agent_services()
        .review()
        .queue_follow_up_review_dispatch(&id, &input.request)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    spawn_review_launch(state.clone(), run.clone());

    Ok(Json(run))
}

#[derive(Debug, Serialize)]
pub(crate) struct DeleteReviewResponse {
    ok: bool,
}

pub(crate) async fn delete_review(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<DeleteReviewResponse>, ApiError> {
    state
        .remote_agent_services()
        .review()
        .delete_review(&id)
        .await
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(DeleteReviewResponse { ok: true }))
}

pub(crate) async fn cancel_review_dispatch(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<ReviewId>,
) -> Result<Json<ReviewRunRecord>, ApiError> {
    let canceled_dispatch = state
        .remote_agent_services()
        .review()
        .cancel_dispatch(&id)
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(canceled_dispatch))
}

pub(crate) fn spawn_review_launch(state: AppState, queued_dispatch: ReviewRunRecord) {
    tokio::spawn(async move {
        let launch_result = state
            .remote_agent_services()
            .review()
            .launch_prepared_review(queued_dispatch.clone())
            .await;

        if let Err(join_error) = launch_result {
            if let Some(mut saved_dispatch) = state
                .database
                .review_dispatch_repository()
                .get_dispatch(&queued_dispatch.review_id, &queued_dispatch.dispatch_id)
                .await
                .ok()
                .flatten()
            {
                if saved_dispatch.status.is_active() {
                    saved_dispatch.status = track_types::types::DispatchStatus::Failed;
                    saved_dispatch.updated_at = now_utc();
                    saved_dispatch.finished_at = Some(saved_dispatch.updated_at);
                    saved_dispatch.error_message = Some(format!(
                        "Background review task stopped unexpectedly: {join_error}"
                    ));
                    let _ = state
                        .database
                        .review_dispatch_repository()
                        .save_dispatch(&saved_dispatch)
                        .await;
                }
            }
        }
    });
}
