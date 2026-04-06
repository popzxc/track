use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use track_config::config::{
    RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile, DEFAULT_REMOTE_AGENT_PORT,
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT, DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
};
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{RemoteAgentPreferredTool, RemoteCleanupSummary, RemoteResetSummary};

use crate::api_error::ApiError;
use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteAgentSettingsResponse {
    configured: bool,
    preferred_tool: RemoteAgentPreferredTool,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell_prelude: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteAgentReviewFollowUpSettingsResponse {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    main_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_review_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteCleanupResponse {
    summary: RemoteCleanupSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteResetResponse {
    summary: RemoteResetSummary,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PutRemoteAgentInput {
    host: String,
    user: String,
    #[serde(default = "default_remote_agent_port")]
    port: u16,
    #[serde(default = "default_remote_agent_workspace_root")]
    workspace_root: String,
    #[serde(default = "default_remote_projects_registry_path")]
    projects_registry_path: String,
    #[serde(default)]
    preferred_tool: RemoteAgentPreferredTool,
    shell_prelude: Option<String>,
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
    ssh_private_key: String,
    known_hosts: Option<String>,
}

fn default_remote_agent_port() -> u16 {
    DEFAULT_REMOTE_AGENT_PORT
}

fn default_remote_agent_workspace_root() -> String {
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned()
}

fn default_remote_projects_registry_path() -> String {
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateRemoteAgentSettingsInput {
    #[serde(default)]
    preferred_tool: Option<RemoteAgentPreferredTool>,
    shell_prelude: String,
    review_follow_up: Option<RemoteAgentReviewFollowUpSettingsResponse>,
}

pub(crate) async fn get_remote_agent_settings(
    State(state): State<AppState>,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let remote_agent = state
        .config_service
        .load_remote_agent_config()
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(remote_agent)))
}

pub(crate) async fn patch_remote_agent_settings(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let input = serde_json::from_slice::<UpdateRemoteAgentSettingsInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;
    let existing_remote_agent = state
        .config_service
        .load_remote_agent_config()
        .await
        .map_err(ApiError::from_track_error)?
        .ok_or_else(|| ApiError::from_track_error(TrackError::new(
            ErrorCode::RemoteAgentNotConfigured,
            "Remote dispatch is not configured yet. Run `track remote-agent configure ...` locally to register the remote host and SSH key first.",
        )))?;

    let remote_agent = state
        .config_service
        .save_remote_agent_settings(
            input
                .preferred_tool
                .unwrap_or(existing_remote_agent.preferred_tool),
            Some(input.shell_prelude),
            input
                .review_follow_up
                .map(|review_follow_up| RemoteAgentReviewFollowUpConfigFile {
                    enabled: review_follow_up.enabled,
                    main_user: review_follow_up.main_user,
                    default_review_prompt: review_follow_up.default_review_prompt,
                })
                .or(existing_remote_agent.review_follow_up),
        )
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(Some(remote_agent))))
}

pub(crate) async fn put_remote_agent_settings(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<RemoteAgentSettingsResponse>, ApiError> {
    let input = serde_json::from_slice::<PutRemoteAgentInput>(&body)
        .map_err(|_| ApiError::invalid_json("Request body is not valid JSON."))?;

    let remote_agent = state
        .config_service
        .replace_remote_agent_config(
            RemoteAgentConfigFile {
                host: input.host,
                user: input.user,
                port: input.port,
                workspace_root: input.workspace_root,
                projects_registry_path: input.projects_registry_path,
                preferred_tool: input.preferred_tool,
                shell_prelude: input.shell_prelude,
                review_follow_up: input.review_follow_up.map(|review_follow_up| {
                    RemoteAgentReviewFollowUpConfigFile {
                        enabled: review_follow_up.enabled,
                        main_user: review_follow_up.main_user,
                        default_review_prompt: review_follow_up.default_review_prompt,
                    }
                }),
            },
            &input.ssh_private_key,
            input.known_hosts.as_deref(),
        )
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(remote_agent_settings_response(Some(remote_agent))))
}

fn remote_agent_settings_response(
    remote_agent: Option<RemoteAgentConfigFile>,
) -> RemoteAgentSettingsResponse {
    match remote_agent {
        Some(remote_agent) => RemoteAgentSettingsResponse {
            configured: true,
            preferred_tool: remote_agent.preferred_tool,
            host: Some(remote_agent.host),
            user: Some(remote_agent.user),
            port: Some(remote_agent.port),
            shell_prelude: remote_agent.shell_prelude,
            review_follow_up: Some(
                remote_agent
                    .review_follow_up
                    .map(
                        |review_follow_up| RemoteAgentReviewFollowUpSettingsResponse {
                            enabled: review_follow_up.enabled,
                            main_user: review_follow_up.main_user,
                            default_review_prompt: review_follow_up.default_review_prompt,
                        },
                    )
                    .unwrap_or(RemoteAgentReviewFollowUpSettingsResponse {
                        enabled: false,
                        main_user: None,
                        default_review_prompt: None,
                    }),
            ),
        },
        None => RemoteAgentSettingsResponse {
            configured: false,
            preferred_tool: RemoteAgentPreferredTool::Codex,
            host: None,
            user: None,
            port: None,
            shell_prelude: None,
            review_follow_up: None,
        },
    }
}

pub(crate) async fn cleanup_remote_agent_artifacts(
    State(state): State<AppState>,
) -> Result<Json<RemoteCleanupResponse>, ApiError> {
    let summary = state
        .remote_agent_services()
        .cleanup_unused_remote_artifacts()
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(RemoteCleanupResponse { summary }))
}

pub(crate) async fn reset_remote_agent_workspace(
    State(state): State<AppState>,
) -> Result<Json<RemoteResetResponse>, ApiError> {
    let summary = state
        .remote_agent_services()
        .reset_remote_workspace()
        .await
        .map_err(ApiError::from_track_error)?;

    Ok(Json(RemoteResetResponse { summary }))
}
