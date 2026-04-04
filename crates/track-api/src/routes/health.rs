use axum::Json;
use serde::Serialize;

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    ok: bool,
}
