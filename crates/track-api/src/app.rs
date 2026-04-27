use std::path::Path;
use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::Request;
use axum::routing::{get, patch, post, put};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use track_types::errors::ErrorCode;
use track_types::time_utils::now_utc;

use crate::api_error::ApiError;
use crate::routes;

use crate::AppState;

pub(crate) fn bump_task_change_version(state: &AppState) -> u64 {
    state.task_change_version.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn spawn_remote_review_follow_up_reconciler(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let reconciliation_run_id =
                format!("review-follow-up-{}", now_utc().unix_timestamp_nanos());

            let reconciliation = {
                let _remote_agent_operation_guard = state.remote_agent_operation_guard().await;
                let runtime_services = match state.remote_agent_runtime_services().await {
                    Ok(runtime_services) => runtime_services,
                    Err(error) if error.code == ErrorCode::RemoteAgentNotConfigured => {
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            reconciliation_run_id = %reconciliation_run_id,
                            "Review follow-up reconciliation failed: {error}"
                        );
                        continue;
                    }
                };
                match runtime_services
                    .review_follow_up()
                    .reconcile_review_follow_up()
                    .await
                {
                    Ok(reconciliation) => reconciliation,
                    Err(error) => {
                        tracing::warn!(
                            reconciliation_run_id = %reconciliation_run_id,
                            "Review follow-up reconciliation failed: {error}"
                        );
                        continue;
                    }
                }
            };

            for event in &reconciliation.events {
                let branch_name = event.branch_name.as_deref().unwrap_or("");
                let pull_request_url = event
                    .pull_request_url
                    .as_ref()
                    .map(|url| url.as_str())
                    .unwrap_or("");
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
                crate::app::bump_task_change_version(&state);
            }

            for queued_dispatch in reconciliation.queued_dispatches {
                // TODO: Right dislocation?
                routes::tasks::spawn_dispatch_launch(state.clone(), queued_dispatch);
            }
        }
    });
}

pub fn build_app(state: AppState, static_root: impl AsRef<Path>) -> Router {
    // The deployed app still serves both API routes and the frontend from one
    // process so Docker can expose a single local port.
    let static_root = static_root.as_ref().to_path_buf();
    let api_router = Router::new()
        .route(
            "/meta/server_version",
            get(routes::meta::get_server_version),
        )
        .route("/projects", get(routes::projects::list_projects))
        .route(
            "/projects/{canonical_name}",
            put(routes::projects::put_project).patch(routes::projects::patch_project),
        )
        .route(
            "/remote-agent",
            get(routes::remote_agent::get_remote_agent_settings)
                .put(routes::remote_agent::put_remote_agent_settings)
                .patch(routes::remote_agent::patch_remote_agent_settings),
        )
        .route(
            "/remote-agent/cleanup",
            post(routes::remote_agent::cleanup_remote_agent_artifacts),
        )
        .route(
            "/remote-agent/reset",
            post(routes::remote_agent::reset_remote_agent_workspace),
        )
        .route("/dispatches", get(routes::dispatches::list_dispatches))
        .route(
            "/reviews",
            get(routes::reviews::list_reviews).post(routes::reviews::create_review),
        )
        .route(
            "/reviews/{id}",
            axum::routing::delete(routes::reviews::delete_review),
        )
        .route("/reviews/{id}/runs", get(routes::reviews::list_review_runs))
        .route(
            "/reviews/{id}/follow-up",
            post(routes::reviews::follow_up_review),
        )
        .route(
            "/reviews/{id}/cancel",
            post(routes::reviews::cancel_review_dispatch),
        )
        .route("/runs", get(routes::runs::list_runs))
        .route(
            "/tasks",
            get(routes::tasks::list_tasks).post(routes::tasks::create_task),
        )
        .route("/tasks/{id}/runs", get(routes::tasks::list_task_runs))
        .route(
            "/tasks/{id}",
            patch(routes::tasks::patch_task).delete(routes::tasks::delete_task),
        )
        .route(
            "/tasks/{id}/dispatch",
            post(routes::tasks::dispatch_task).delete(routes::tasks::discard_task_dispatch),
        )
        .route("/tasks/{id}/follow-up", post(routes::tasks::follow_up_task))
        .route(
            "/tasks/{id}/dispatch/cancel",
            post(routes::tasks::cancel_task_dispatch),
        )
        .route(
            "/events/version",
            get(routes::events::get_task_change_version),
        )
        .route(
            "/events/tasks-changed",
            axum::routing::post(routes::events::notify_task_change),
        )
        .fallback(async || ApiError::not_found());

    Router::new()
        .route("/health", get(routes::health::health))
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
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    let matched_path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str)
                        .unwrap_or(request.uri().path());
                    tracing::span!(
                        tracing::Level::INFO,
                        "http_request",
                        method = %request.method(),
                        matched_path = %matched_path,
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_request(|_request: &Request<Body>, _span: &tracing::Span| {
                    tracing::info!("API request started");
                })
                .on_response(
                    |response: &axum::http::Response<Body>,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        let status = response.status();
                        if status.is_server_error() {
                            tracing::error!(
                                status = %status,
                                latency_ms = latency.as_millis(),
                                "API request failed"
                            );
                        } else if status.is_client_error() {
                            tracing::warn!(
                                status = %status,
                                latency_ms = latency.as_millis(),
                                "API request completed with client error"
                            );
                        } else {
                            tracing::info!(
                                status = %status,
                                latency_ms = latency.as_millis(),
                                "API request completed"
                            );
                        }
                    },
                )
                .on_failure(
                    |error: tower_http::classify::ServerErrorsFailureClass,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        tracing::error!(
                            classification = ?error,
                            latency_ms = latency.as_millis(),
                            "API request encountered an internal failure"
                        );
                    },
                ),
        )
        .with_state(state)
}
