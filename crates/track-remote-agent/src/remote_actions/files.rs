use std::env;
use std::fs;

use track_config::paths::path_to_string;
use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::now_utc;

use crate::scripts::{PrepareRemoteUploadScript, ReadRemoteFileScript};
use crate::ssh::{ScriptOutput, SshClient};

/// Reads one remote file while preserving the difference between "this file
/// does not exist yet" and "the remote command failed".
pub(crate) struct ReadRemoteFileAction<'a> {
    ssh_client: &'a SshClient,
    remote_path: &'a str,
}

impl<'a> ReadRemoteFileAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, remote_path: &'a str) -> Self {
        Self {
            ssh_client,
            remote_path,
        }
    }

    pub(crate) fn execute(&self) -> Result<Option<String>, TrackError> {
        let script = ReadRemoteFileScript;
        let arguments = script.arguments(self.remote_path);
        match self
            .ssh_client
            .run_script_with_exit_code(&script.render(), &arguments)?
        {
            ScriptOutput::Success(stdout) => Ok(Some(stdout)),
            ScriptOutput::ExitCode(ReadRemoteFileScript::MISSING_FILE_EXIT_CODE) => Ok(None),
            ScriptOutput::ExitCode(code) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Could not read the remote file at {}: remote command exited with status code {code}.",
                    self.remote_path
                ),
            )),
            ScriptOutput::Failure(stderr) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Could not read the remote file at {}: {stderr}",
                    self.remote_path
                ),
            )),
        }
    }
}

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
