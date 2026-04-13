use track_types::errors::{ErrorCode, TrackError};

use crate::helper::{ListDirectoriesRequest, ListDirectoriesResponse};
use crate::ssh::SshClient;

// =============================================================================
// Remote Filesystem Helpers
// =============================================================================
//
// The remote view only needs a tiny read-only subset of shell access today.
// These helpers keep that contract explicit so the rest of the module can talk
// in terms of "list directories" instead of embedding ad-hoc shell snippets.
pub(super) fn list_directories(
    ssh_client: &SshClient,
    remote_path: &str,
) -> Result<Vec<String>, TrackError> {
    let response = ssh_client.run_helper_json::<_, ListDirectoriesResponse>(
        "list-directories",
        &ListDirectoriesRequest { path: remote_path },
    )?;

    response
        .paths
        .into_iter()
        .map(|path| {
            let trimmed = path.trim().to_owned();
            if trimmed.is_empty() {
                return Err(TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote directory listing for {remote_path} contained an empty path."),
                ));
            }

            Ok(trimmed)
        })
        .collect()
}
