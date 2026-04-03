use track_types::errors::{ErrorCode, TrackError};

use crate::remote_actions::{ReadRemoteFileAction, UploadRemoteFileAction};
use crate::ssh::SshClient;
use crate::types::RemoteProjectRegistryFile;

/// Loads the remote project registry and falls back to an empty registry when
/// the remote machine has not created one yet.
pub(crate) struct LoadRemoteRegistryAction<'a> {
    ssh_client: &'a SshClient,
    registry_path: &'a str,
}

impl<'a> LoadRemoteRegistryAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, registry_path: &'a str) -> Self {
        Self {
            ssh_client,
            registry_path,
        }
    }

    pub(crate) fn execute(&self) -> Result<RemoteProjectRegistryFile, TrackError> {
        let Some(raw_registry) =
            ReadRemoteFileAction::new(self.ssh_client, self.registry_path).execute()?
        else {
            return Ok(RemoteProjectRegistryFile::default());
        };

        serde_json::from_str::<RemoteProjectRegistryFile>(&raw_registry).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Remote projects registry is not valid JSON: {error}"),
            )
        })
    }
}

/// Persists the remote project registry as canonical JSON so later runs can
/// discover reusable checkouts and fork metadata from a stable remote file.
pub(crate) struct WriteRemoteRegistryAction<'a> {
    ssh_client: &'a SshClient,
    registry_path: &'a str,
    registry: &'a RemoteProjectRegistryFile,
}

impl<'a> WriteRemoteRegistryAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        registry_path: &'a str,
        registry: &'a RemoteProjectRegistryFile,
    ) -> Self {
        Self {
            ssh_client,
            registry_path,
            registry,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let serialized = serde_json::to_string_pretty(self.registry).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Could not serialize the remote projects registry: {error}"),
            )
        })?;

        UploadRemoteFileAction::new(self.ssh_client, self.registry_path, &serialized).execute()
    }
}
