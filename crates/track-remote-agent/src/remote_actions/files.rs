use track_types::errors::TrackError;

use crate::helper::{EmptyResponse, WriteFileRequest};
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
        self.ssh_client
            .run_helper_json::<_, EmptyResponse>(
                "write-file",
                &WriteFileRequest {
                    path: self.remote_path,
                    contents: self.contents,
                },
            )
            .map(|_| ())
    }
}
