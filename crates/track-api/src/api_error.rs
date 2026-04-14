use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use track_types::errors::{ErrorCode, TrackError};

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Debug, Serialize)]
struct ApiErrorPayload {
    code: String,
    message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    pub fn from_track_error(error: TrackError) -> Self {
        let status = match error.code {
            ErrorCode::TaskNotFound => StatusCode::NOT_FOUND,
            ErrorCode::InvalidJson
            | ErrorCode::InvalidGitRemote
            | ErrorCode::InvalidPathComponent
            | ErrorCode::InvalidProjectMetadata
            | ErrorCode::InvalidRemoteAgentConfig
            | ErrorCode::InvalidTaskUpdate
            | ErrorCode::VersionMismatch
            | ErrorCode::ConfigNotFound
            | ErrorCode::InvalidConfig
            | ErrorCode::InvalidConfigInput
            | ErrorCode::NoProjectRoots
            | ErrorCode::NoProjectsDiscovered
            | ErrorCode::InvalidProjectSelection
            | ErrorCode::AiParseFailed
            | ErrorCode::EmptyInput
            | ErrorCode::InteractiveRequired
            | ErrorCode::DispatchWriteFailed
            | ErrorCode::RemoteAgentNotConfigured
            | ErrorCode::ProjectWriteFailed
            | ErrorCode::TaskWriteFailed => StatusCode::BAD_REQUEST,
            ErrorCode::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::ProjectNotFound | ErrorCode::DispatchNotFound => StatusCode::NOT_FOUND,
            ErrorCode::RemoteDispatchFailed => StatusCode::BAD_GATEWAY,
        };

        Self {
            status,
            code: error.code.to_string(),
            message: error.to_string(),
        }
    }

    pub fn invalid_json(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::InvalidJson.to_string(),
            message: message.to_owned(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR".to_owned(),
            message: message.into(),
        }
    }

    pub fn not_found() -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code: "ROUTE_NOT_FOUND".to_owned(),
            message: "Route was not found.".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::warn!(
            status = %self.status,
            error_code = %self.code,
            error_message = %self.message,
            "API request returned an error response"
        );

        (
            self.status,
            Json(ApiErrorBody {
                error: ApiErrorPayload {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}
