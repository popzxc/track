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
use track_remote_agent::RemoteAgentConfigProvider;
use track_types::errors::{ErrorCode, TrackError};
use track_types::migration::{MigrationStatus, MIGRATION_STATUS_SETTING_KEY};
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
                "Remote dispatch is not configured yet. Import legacy data or register remote-agent settings first.",
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

    pub async fn load_migration_status(&self) -> Result<MigrationStatus, TrackError> {
        Ok(self
            .database
            .settings_repository()
            .load_json(MIGRATION_STATUS_SETTING_KEY)
            .await?
            .unwrap_or_else(MigrationStatus::ready))
    }

    pub async fn save_migration_status(&self, status: &MigrationStatus) -> Result<(), TrackError> {
        self.database
            .settings_repository()
            .save_json(MIGRATION_STATUS_SETTING_KEY, status)
            .await
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
        self.repository.save_remote_agent_config(config).await
    }

    pub async fn replace_remote_agent_config(
        &self,
        config: RemoteAgentConfigFile,
        ssh_private_key: &str,
        known_hosts: Option<&str>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        self.repository
            .replace_remote_agent_config(config, ssh_private_key, known_hosts)
            .await
    }

    pub async fn save_remote_agent_settings(
        &self,
        preferred_tool: RemoteAgentPreferredTool,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        self.repository
            .save_remote_agent_settings(preferred_tool, shell_prelude, review_follow_up)
            .await
    }

    pub async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        self.load_remote_agent_config()
            .await?
            .map(build_remote_agent_runtime_config)
            .transpose()
    }

    pub async fn load_migration_status(&self) -> Result<MigrationStatus, TrackError> {
        self.repository.load_migration_status().await
    }

    pub async fn save_migration_status(&self, status: &MigrationStatus) -> Result<(), TrackError> {
        self.repository.save_migration_status(status).await
    }
}

#[async_trait::async_trait]
impl RemoteAgentConfigProvider for RemoteAgentConfigService {
    async fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        RemoteAgentConfigService::load_remote_agent_runtime_config(self).await
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::BackendConfigRepository;
    use track_dal::database::DatabaseContext;
    use track_types::migration::{LegacyScanSummary, MigrationState, MigrationStatus};

    async fn repository() -> (TempDir, BackendConfigRepository) {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");

        (
            directory,
            BackendConfigRepository::new(Some(database))
                .await
                .expect("backend config repository should resolve"),
        )
    }

    fn status(state: MigrationState) -> MigrationStatus {
        let requires_migration = matches!(state, MigrationState::ImportRequired);
        MigrationStatus {
            state,
            requires_migration,
            can_import: requires_migration,
            legacy_detected: true,
            summary: LegacyScanSummary::default(),
            skipped_records: Vec::new(),
            cleanup_candidates: Vec::new(),
        }
    }

    #[tokio::test]
    async fn saves_and_loads_imported_status() {
        let (_directory, repository) = repository().await;
        repository
            .save_migration_status(&status(MigrationState::Imported))
            .await
            .expect("migration status should save");

        let loaded = repository
            .load_migration_status()
            .await
            .expect("migration status should load");

        assert_eq!(loaded.state, MigrationState::Imported);
        assert!(!loaded.requires_migration);
        assert!(!loaded.can_import);
    }
}
