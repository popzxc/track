use std::fs;

use track_config::config::{
    canonicalize_remote_agent_config, RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile,
};
use track_config::paths::{
    collapse_home_path, get_backend_managed_remote_agent_key_path,
    get_backend_managed_remote_agent_known_hosts_path,
};
use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::database::DatabaseContext;
use track_remote_agent::invalidate_helper_upload;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteAgentPreferredTool;

pub(crate) const REMOTE_AGENT_SETTING_KEY: &str = "remote_agent_config";

#[derive(Debug, Clone)]
pub struct BackendConfigRepository {
    database: DatabaseContext,
}

impl BackendConfigRepository {
    pub async fn new(database: Option<DatabaseContext>) -> Result<Self, TrackError> {
        let database = match database {
            Some(database) => database,
            None => DatabaseContext::initialized(None).await?,
        };

        Ok(Self { database })
    }

    pub async fn load_remote_agent_config(
        &self,
    ) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        self.database
            .settings_repository()
            .load_json(REMOTE_AGENT_SETTING_KEY)
            .await
    }

    pub async fn save_remote_agent_config(
        &self,
        config: Option<&RemoteAgentConfigFile>,
    ) -> Result<(), TrackError> {
        match config {
            Some(config) => {
                let canonical = canonicalize_remote_agent_config(config.clone())?;
                self.database
                    .settings_repository()
                    .save_json(REMOTE_AGENT_SETTING_KEY, &canonical)
                    .await
            }
            None => {
                self.database
                    .settings_repository()
                    .delete(REMOTE_AGENT_SETTING_KEY)
                    .await
            }
        }
    }

    pub async fn replace_remote_agent_config(
        &self,
        config: RemoteAgentConfigFile,
        ssh_private_key: &str,
        known_hosts: Option<&str>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let canonical = canonicalize_remote_agent_config(config)?;
        install_backend_remote_agent_secrets(ssh_private_key, known_hosts)?;
        self.database
            .settings_repository()
            .save_json(REMOTE_AGENT_SETTING_KEY, &canonical)
            .await?;
        Ok(canonical)
    }

    pub async fn save_remote_agent_settings(
        &self,
        preferred_tool: RemoteAgentPreferredTool,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let mut config = self.load_remote_agent_config().await?.ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote dispatch is not configured yet. Register remote-agent settings first.",
            )
        })?;

        config.preferred_tool = preferred_tool;
        config.shell_prelude = shell_prelude
            .map(|value| value.replace("\r\n", "\n").trim().to_owned())
            .filter(|value| !value.is_empty());
        config.review_follow_up = review_follow_up;
        self.save_remote_agent_config(Some(&config)).await?;

        Ok(config)
    }
}

#[derive(Debug, Clone)]
pub struct RemoteAgentConfigService {
    repository: BackendConfigRepository,
}

impl RemoteAgentConfigService {
    pub async fn new(repository: Option<BackendConfigRepository>) -> Result<Self, TrackError> {
        let repository = match repository {
            Some(repository) => repository,
            None => BackendConfigRepository::new(None).await?,
        };

        Ok(Self { repository })
    }

    pub async fn load_remote_agent_config(
        &self,
    ) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        self.repository.load_remote_agent_config().await
    }

    pub async fn save_remote_agent_config(
        &self,
        config: Option<&RemoteAgentConfigFile>,
    ) -> Result<(), TrackError> {
        self.repository.save_remote_agent_config(config).await?;
        invalidate_helper_upload();
        Ok(())
    }

    pub async fn replace_remote_agent_config(
        &self,
        config: RemoteAgentConfigFile,
        ssh_private_key: &str,
        known_hosts: Option<&str>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let config = self
            .repository
            .replace_remote_agent_config(config, ssh_private_key, known_hosts)
            .await?;
        invalidate_helper_upload();
        Ok(config)
    }

    pub async fn save_remote_agent_settings(
        &self,
        preferred_tool: RemoteAgentPreferredTool,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let config = self
            .repository
            .save_remote_agent_settings(preferred_tool, shell_prelude, review_follow_up)
            .await?;
        invalidate_helper_upload();
        Ok(config)
    }

    pub async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        self.load_remote_agent_config()
            .await?
            .map(build_remote_agent_runtime_config)
            .transpose()
    }
}

fn build_remote_agent_runtime_config(
    config: RemoteAgentConfigFile,
) -> Result<RemoteAgentRuntimeConfig, TrackError> {
    Ok(RemoteAgentRuntimeConfig {
        host: config.host,
        user: config.user,
        port: config.port,
        workspace_root: config.workspace_root,
        projects_registry_path: config.projects_registry_path,
        preferred_tool: config.preferred_tool,
        shell_prelude: config.shell_prelude,
        review_follow_up: config.review_follow_up.and_then(|review_follow_up| {
            review_follow_up
                .main_user
                .map(|main_user| RemoteAgentReviewFollowUpRuntimeConfig {
                    enabled: review_follow_up.enabled,
                    main_user,
                    default_review_prompt: review_follow_up.default_review_prompt,
                })
        }),
        managed_key_path: get_backend_managed_remote_agent_key_path()?,
        managed_known_hosts_path: get_backend_managed_remote_agent_known_hosts_path()?,
    })
}

fn install_backend_remote_agent_secrets(
    ssh_private_key: &str,
    known_hosts: Option<&str>,
) -> Result<(), TrackError> {
    let managed_key_path = get_backend_managed_remote_agent_key_path()?;
    let known_hosts_path = get_backend_managed_remote_agent_known_hosts_path()?;
    let Some(parent_directory) = managed_key_path.parent() else {
        return Err(TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            "Could not determine the backend remote-agent secrets directory.",
        ));
    };

    fs::create_dir_all(parent_directory).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            format!(
                "Could not create the backend remote-agent secrets directory at {}: {error}",
                collapse_home_path(parent_directory)
            ),
        )
    })?;

    let normalized_private_key = ssh_private_key.replace("\r\n", "\n");
    if normalized_private_key.trim().is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            "Remote agent setup requires a non-empty SSH private key.",
        ));
    }

    fs::write(&managed_key_path, normalized_private_key).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            format!(
                "Could not write the managed SSH private key at {}: {error}",
                collapse_home_path(&managed_key_path)
            ),
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&managed_key_path, fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    format!(
                        "Could not set permissions on the managed SSH private key at {}: {error}",
                        collapse_home_path(&managed_key_path)
                    ),
                )
            },
        )?;
    }

    match known_hosts {
        Some(known_hosts) => {
            fs::write(&known_hosts_path, known_hosts.replace("\r\n", "\n")).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    format!(
                        "Could not write the managed known_hosts file at {}: {error}",
                        collapse_home_path(&known_hosts_path)
                    ),
                )
            })?;
        }
        None if !known_hosts_path.exists() => {
            fs::write(&known_hosts_path, "").map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    format!(
                        "Could not create the managed known_hosts file at {}: {error}",
                        collapse_home_path(&known_hosts_path)
                    ),
                )
            })?;
        }
        None => {}
    }

    Ok(())
}
