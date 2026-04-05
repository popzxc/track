use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use track_projects::project_metadata::{
    ProjectMetadataUpdateInput, ProjectRecord, ProjectUpsertInput,
};

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Serialize)]
pub(crate) struct ProjectsResponse {
    projects: Vec<ProjectRecord>,
}

pub(crate) async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<ProjectsResponse>, ApiError> {
    let projects = state
        .project_repository
        .list_projects()
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(ProjectsResponse { projects }))
}

pub(crate) async fn patch_project(
    State(state): State<AppState>,
    AxumPath(canonical_name): AxumPath<String>,
    body: Bytes,
) -> Result<Json<ProjectRecord>, ApiError> {
    let input = serde_json::from_slice::<ProjectMetadataUpdateInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let project = state
        .project_repository
        .update_project_by_name(
            &canonical_name,
            input.validate().map_err(ApiError::from_track_error)?,
        )
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(project))
}

#[derive(Debug, Deserialize)]
pub(crate) struct PutProjectInput {
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(flatten)]
    metadata: ProjectMetadataUpdateInput,
}

pub(crate) async fn put_project(
    State(state): State<AppState>,
    AxumPath(canonical_name): AxumPath<String>,
    body: Bytes,
) -> Result<Json<ProjectRecord>, ApiError> {
    let input = serde_json::from_slice::<PutProjectInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let project = state
        .project_repository
        .upsert_project(ProjectUpsertInput {
            canonical_name,
            aliases: input.aliases,
            metadata: input.metadata,
        })
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(project))
}
