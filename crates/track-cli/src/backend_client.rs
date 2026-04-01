use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use track_core::build_info::BuildInfo;
use track_core::errors::{ErrorCode, TrackError};
use track_core::migration::{MigrationImportSummary, MigrationStatus};
use track_core::project_repository::{ProjectMetadata, ProjectRecord};
use track_core::types::{Task, TaskCreateInput};

use crate::build_info::cli_build_info;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SERVER_VERSION_PATH: &str = "/api/meta/server_version";

pub trait TrackBackend {
    fn fetch_projects(&self) -> Result<Vec<ProjectRecord>, TrackError>;
    fn create_task(&self, input: &TaskCreateInput) -> Result<Task, TrackError>;
    fn migration_status(&self) -> Result<MigrationStatus, TrackError>;
    fn import_legacy_data(&self) -> Result<MigrationImportSummary, TrackError>;
    fn configure_remote_agent(
        &self,
        input: &ConfigureRemoteAgentRequest,
    ) -> Result<RemoteAgentSettingsResponse, TrackError>;
    fn register_project(
        &self,
        canonical_name: &str,
        aliases: Vec<String>,
        metadata: ProjectMetadata,
    ) -> Result<ProjectRecord, TrackError>;
}

#[derive(Debug, Clone)]
pub struct HttpTrackBackend {
    base_url: String,
    agent: ureq::Agent,
    version_match_verified: Arc<OnceLock<()>>,
}

impl HttpTrackBackend {
    pub fn new(base_url: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();

        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            agent,
            version_match_verified: Arc::new(OnceLock::new()),
        }
    }

    fn get_json<T>(&self, path: &str) -> Result<T, TrackError>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.send::<(), T>("GET", path, None)
    }

    fn post_json<B, T>(&self, path: &str, body: Option<&B>) -> Result<T, TrackError>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        self.send("POST", path, body)
    }

    fn put_json<B, T>(&self, path: &str, body: &B) -> Result<T, TrackError>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        self.send("PUT", path, Some(body))
    }

    fn send<B, T>(&self, method: &str, path: &str, body: Option<&B>) -> Result<T, TrackError>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        if path != SERVER_VERSION_PATH {
            self.ensure_server_version_match()?;
        }

        self.send_unchecked(method, path, body)
    }

    // The CLI talks only to the local server, so one successful handshake is
    // enough for the rest of the process lifetime. We intentionally skip
    // caching failures so a restarted server can be retried on the next
    // command invocation without extra state management here.
    fn ensure_server_version_match(&self) -> Result<(), TrackError> {
        if self.version_match_verified.get().is_some() {
            return Ok(());
        }

        let server_build =
            self.send_unchecked::<(), BuildInfo>("GET", SERVER_VERSION_PATH, None)?;
        let cli_build = cli_build_info();
        if !cli_build.matches_release(&server_build) {
            return Err(version_mismatch_error(&cli_build, &server_build));
        }

        let _ = self.version_match_verified.set(());
        Ok(())
    }

    fn send_unchecked<B, T>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, TrackError>
    where
        B: Serialize,
        T: for<'de> Deserialize<'de>,
    {
        let url = format!("{}{}", self.base_url, path);
        let response = match (method, body) {
            ("GET", None) => self.agent.get(&url).call(),
            ("POST", None) => self.agent.post(&url).send_empty(),
            ("POST", Some(body)) => {
                let serialized = serde_json::to_string(body).map_err(|error| {
                    TrackError::new(
                        ErrorCode::InvalidJson,
                        format!("Could not serialize the backend request body: {error}"),
                    )
                })?;
                self.agent
                    .post(&url)
                    .header("content-type", "application/json")
                    .send(serialized)
            }
            ("PUT", Some(body)) => {
                let serialized = serde_json::to_string(body).map_err(|error| {
                    TrackError::new(
                        ErrorCode::InvalidJson,
                        format!("Could not serialize the backend request body: {error}"),
                    )
                })?;
                self.agent
                    .put(&url)
                    .header("content-type", "application/json")
                    .send(serialized)
            }
            _ => {
                return Err(TrackError::new(
                    ErrorCode::InvalidConfigInput,
                    format!("Unsupported backend request method `{method}`."),
                ))
            }
        }
        .map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Could not reach the track backend at {}: {error}",
                    self.base_url
                ),
            )
        })?;

        let status = response.status();
        if !(200..300).contains(&status.as_u16()) {
            let body = response.into_body().read_to_string().unwrap_or_default();
            let api_error = serde_json::from_str::<ApiErrorBody>(&body).ok();
            let (code, message) = match api_error {
                Some(api_error) => (
                    map_api_error_code(&api_error.error.code),
                    api_error.error.message,
                ),
                None => (
                    ErrorCode::InvalidConfigInput,
                    format!("Backend request to {path} failed with status {status}."),
                ),
            };

            return Err(TrackError::new(code, message));
        }

        let body = response.into_body().read_to_string().map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidJson,
                format!("Could not read the backend response body from {path}: {error}"),
            )
        })?;

        serde_json::from_str::<T>(&body).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidJson,
                format!("Backend response from {path} is not valid JSON: {error}"),
            )
        })
    }
}

impl TrackBackend for HttpTrackBackend {
    fn fetch_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
        Ok(self.get_json::<ProjectsResponse>("/api/projects")?.projects)
    }

    fn create_task(&self, input: &TaskCreateInput) -> Result<Task, TrackError> {
        self.post_json("/api/tasks", Some(input))
    }

    fn migration_status(&self) -> Result<MigrationStatus, TrackError> {
        Ok(self
            .get_json::<MigrationStatusResponse>("/api/migration/status")?
            .migration)
    }

    fn import_legacy_data(&self) -> Result<MigrationImportSummary, TrackError> {
        Ok(self
            .post_json::<serde_json::Value, MigrationImportResponse>("/api/migration/import", None)?
            .summary)
    }

    fn register_project(
        &self,
        canonical_name: &str,
        aliases: Vec<String>,
        metadata: ProjectMetadata,
    ) -> Result<ProjectRecord, TrackError> {
        self.put_json(
            &format!("/api/projects/{canonical_name}"),
            &RegisterProjectRequest { aliases, metadata },
        )
    }

    fn configure_remote_agent(
        &self,
        input: &ConfigureRemoteAgentRequest,
    ) -> Result<RemoteAgentSettingsResponse, TrackError> {
        self.put_json("/api/remote-agent", input)
    }
}

#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    projects: Vec<ProjectRecord>,
}

#[derive(Debug, Deserialize)]
struct MigrationStatusResponse {
    migration: MigrationStatus,
}

#[derive(Debug, Deserialize)]
struct MigrationImportResponse {
    summary: MigrationImportSummary,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Debug, Deserialize)]
struct ApiErrorPayload {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct RegisterProjectRequest {
    aliases: Vec<String>,
    #[serde(flatten)]
    metadata: ProjectMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigureRemoteAgentRequest {
    pub host: String,
    pub user: String,
    pub port: u16,
    #[serde(rename = "workspaceRoot")]
    pub workspace_root: String,
    #[serde(rename = "projectsRegistryPath")]
    pub projects_registry_path: String,
    #[serde(rename = "shellPrelude", skip_serializing_if = "Option::is_none")]
    pub shell_prelude: Option<String>,
    #[serde(rename = "reviewFollowUp", skip_serializing_if = "Option::is_none")]
    pub review_follow_up: Option<ConfigureRemoteAgentReviewFollowUpRequest>,
    #[serde(rename = "sshPrivateKey")]
    pub ssh_private_key: String,
    #[serde(rename = "knownHosts", skip_serializing_if = "Option::is_none")]
    pub known_hosts: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigureRemoteAgentReviewFollowUpRequest {
    pub enabled: bool,
    #[serde(rename = "mainUser", skip_serializing_if = "Option::is_none")]
    pub main_user: Option<String>,
    #[serde(
        rename = "defaultReviewPrompt",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_review_prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteAgentSettingsResponse {
    pub configured: bool,
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
}

fn map_api_error_code(code: &str) -> ErrorCode {
    match code {
        "MIGRATION_REQUIRED" => ErrorCode::MigrationRequired,
        "PROJECT_NOT_FOUND" => ErrorCode::ProjectNotFound,
        "INVALID_PROJECT_SELECTION" => ErrorCode::InvalidProjectSelection,
        "INVALID_JSON" => ErrorCode::InvalidJson,
        "INVALID_REMOTE_AGENT_CONFIG" => ErrorCode::InvalidRemoteAgentConfig,
        "INVALID_TASK_UPDATE" => ErrorCode::InvalidTaskUpdate,
        "VERSION_MISMATCH" => ErrorCode::VersionMismatch,
        "TASK_NOT_FOUND" => ErrorCode::TaskNotFound,
        "INVALID_CONFIG" => ErrorCode::InvalidConfig,
        "INVALID_CONFIG_INPUT" => ErrorCode::InvalidConfigInput,
        "REMOTE_AGENT_NOT_CONFIGURED" => ErrorCode::RemoteAgentNotConfigured,
        _ => ErrorCode::InvalidConfigInput,
    }
}

fn version_mismatch_error(cli_build: &BuildInfo, server_build: &BuildInfo) -> TrackError {
    TrackError::new(
        ErrorCode::VersionMismatch,
        format!(
            "CLI and WebUI/API versions do not match.\nCLI: {}\nWebUI/API: {}\nRebuild and restart both together before retrying.",
            cli_build.release_label(),
            server_build.release_label(),
        ),
    )
}
