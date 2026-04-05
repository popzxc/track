//! TODO: migration should be no more more

use axum::extract::State;
use axum::middleware::Next;
use axum::Json;
use axum::{extract::Request, response::Response as AxumResponse};
use serde::Serialize;
use track_types::errors::{ErrorCode, TrackError};
use track_types::migration::{MigrationImportSummary, MigrationStatus};

use crate::api_error::ApiError;
use crate::AppState;

pub(crate) async fn migration_status(
    State(state): State<AppState>,
) -> Result<Json<MigrationStatusResponse>, ApiError> {
    let migration = state
        .migration_service
        .status()
        .map_err(ApiError::from_track_error)?;

    Ok(Json(MigrationStatusResponse { migration }))
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationStatusResponse {
    migration: MigrationStatus,
}

pub(crate) async fn import_legacy_data(
    State(state): State<AppState>,
) -> Result<Json<MigrationImportResponse>, ApiError> {
    let summary = state
        .migration_service
        .import_legacy()
        .map_err(ApiError::from_track_error)?;
    crate::app::bump_task_change_version(&state);

    Ok(Json(MigrationImportResponse { summary }))
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationImportResponse {
    summary: MigrationImportSummary,
}

pub(crate) async fn enforce_migration_gate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<AxumResponse, ApiError> {
    let migration = state
        .migration_service
        .status()
        .map_err(ApiError::from_track_error)?;
    if migration.requires_migration {
        return Err(ApiError::from_track_error(TrackError::new(
            ErrorCode::MigrationRequired,
            "Legacy track data must be imported before the backend can serve normal API routes.",
        )));
    }

    Ok(next.run(request).await)
}
