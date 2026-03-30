use std::fs;

use crate::config::{
    canonicalize_remote_agent_config, RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile,
};
use crate::errors::{ErrorCode, TrackError};
use crate::migration::{MigrationStatus, MIGRATION_STATUS_SETTING_KEY};
use crate::paths::{
    collapse_home_path, get_backend_managed_remote_agent_key_path,
    get_backend_managed_remote_agent_known_hosts_path,
};
use crate::settings_repository::SettingsRepository;
use crate::types::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};

pub(crate) const REMOTE_AGENT_SETTING_KEY: &str = "remote_agent_config";

#[derive(Debug, Clone)]
pub struct BackendConfigRepository {
    settings: SettingsRepository,
}

impl BackendConfigRepository {
    pub fn new(settings: Option<SettingsRepository>) -> Result<Self, TrackError> {
        let settings = match settings {
            Some(settings) => settings,
            None => SettingsRepository::new(None)?,
        };

        Ok(Self { settings })
    }

    pub fn load_remote_agent_config(&self) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        self.settings.load_json(REMOTE_AGENT_SETTING_KEY)
    }

    pub fn save_remote_agent_config(
        &self,
        config: Option<&RemoteAgentConfigFile>,
    ) -> Result<(), TrackError> {
        match config {
            Some(config) => {
                let canonical = canonicalize_remote_agent_config(config.clone())?;
                self.settings
                    .save_json(REMOTE_AGENT_SETTING_KEY, &canonical)
            }
            None => self.settings.delete(REMOTE_AGENT_SETTING_KEY),
        }
    }

    pub fn replace_remote_agent_config(
        &self,
        config: RemoteAgentConfigFile,
        ssh_private_key: &str,
        known_hosts: Option<&str>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let canonical = canonicalize_remote_agent_config(config)?;
        install_backend_remote_agent_secrets(ssh_private_key, known_hosts)?;
        self.settings
            .save_json(REMOTE_AGENT_SETTING_KEY, &canonical)?;
        Ok(canonical)
    }

    pub fn save_remote_agent_settings(
        &self,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let mut config = self.load_remote_agent_config()?.ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote dispatch is not configured yet. Import legacy data or register remote-agent settings first.",
            )
        })?;

        config.shell_prelude = shell_prelude
            .map(|value| value.replace("\r\n", "\n").trim().to_owned())
            .filter(|value| !value.is_empty());
        config.review_follow_up = review_follow_up;
        self.save_remote_agent_config(Some(&config))?;

        Ok(config)
    }

    pub fn load_migration_status(&self) -> Result<MigrationStatus, TrackError> {
        Ok(self
            .settings
            .load_json(MIGRATION_STATUS_SETTING_KEY)?
            .unwrap_or_else(MigrationStatus::ready))
    }

    pub fn save_migration_status(&self, status: &MigrationStatus) -> Result<(), TrackError> {
        self.settings
            .save_json(MIGRATION_STATUS_SETTING_KEY, status)
    }

}

#[derive(Debug, Clone)]
pub struct RemoteAgentConfigService {
    repository: BackendConfigRepository,
}

impl RemoteAgentConfigService {
    pub fn new(repository: Option<BackendConfigRepository>) -> Result<Self, TrackError> {
        let repository = match repository {
            Some(repository) => repository,
            None => BackendConfigRepository::new(None)?,
        };

        Ok(Self { repository })
    }

    pub fn load_remote_agent_config(&self) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        self.repository.load_remote_agent_config()
    }

    pub fn save_remote_agent_config(
        &self,
        config: Option<&RemoteAgentConfigFile>,
    ) -> Result<(), TrackError> {
        self.repository.save_remote_agent_config(config)
    }

    pub fn replace_remote_agent_config(
        &self,
        config: RemoteAgentConfigFile,
        ssh_private_key: &str,
        known_hosts: Option<&str>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        self.repository
            .replace_remote_agent_config(config, ssh_private_key, known_hosts)
    }

    pub fn save_remote_agent_settings(
        &self,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        self.repository
            .save_remote_agent_settings(shell_prelude, review_follow_up)
    }

    pub fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        Ok(self
            .load_remote_agent_config()?
            .map(build_remote_agent_runtime_config)
            .transpose()?)
    }

    pub fn load_migration_status(&self) -> Result<MigrationStatus, TrackError> {
        self.repository.load_migration_status()
    }

    pub fn save_migration_status(&self, status: &MigrationStatus) -> Result<(), TrackError> {
        self.repository.save_migration_status(status)
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
    use crate::database::DatabaseContext;
    use crate::migration::{LegacyScanSummary, MigrationState, MigrationStatus};
    use crate::settings_repository::SettingsRepository;

    fn repository() -> (TempDir, BackendConfigRepository) {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::new(Some(directory.path().join("track.sqlite")))
            .expect("database should resolve");
        let settings =
            SettingsRepository::new(Some(database)).expect("settings repository should resolve");

        (
            directory,
            BackendConfigRepository::new(Some(settings))
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

    #[test]
    fn saves_and_loads_imported_status() {
        let (_directory, repository) = repository();
        repository
            .save_migration_status(&status(MigrationState::Imported))
            .expect("migration status should save");

        let loaded = repository
            .load_migration_status()
            .expect("migration status should load");

        assert_eq!(loaded.state, MigrationState::Imported);
        assert!(!loaded.requires_migration);
        assert!(!loaded.can_import);
    }
}
