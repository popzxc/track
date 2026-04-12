use std::env;
use std::fs;

use track_config::paths::path_to_string;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::now_utc;

use crate::scripts::PrepareRemoteUploadScript;
use crate::ssh::SshClient;

/// Uploads one logical artifact to the remote machine after preparing its
/// parent path, so higher layers can treat remote file writes as a single
/// named operation.
pub(crate) struct UploadRemoteFileAction<'a> {
    ssh_client: &'a SshClient,
    remote_path: &'a str,
    contents: &'a str,
}

impl<'a> UploadRemoteFileAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, remote_path: &'a str, contents: &'a str) -> Self {
        Self {
            ssh_client,
            remote_path,
            contents,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = PrepareRemoteUploadScript;
        let arguments = script.arguments(self.remote_path);
        self.ssh_client.run_script(&script.render(), &arguments)?;

        let local_temp_file = env::temp_dir().join(format!(
            "track-remote-upload-{}",
            now_utc().unix_timestamp_nanos()
        ));
        fs::write(&local_temp_file, self.contents).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not write a temporary upload file at {}: {error}",
                    path_to_string(&local_temp_file)
                ),
            )
        })?;

        let upload_result = self
            .ssh_client
            .copy_local_file_to_remote(&local_temp_file, self.remote_path);
        let _ = fs::remove_file(&local_temp_file);

        upload_result
    }
}
