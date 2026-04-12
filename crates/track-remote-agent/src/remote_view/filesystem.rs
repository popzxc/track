use track_types::errors::{ErrorCode, TrackError};

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
    let script = r#"
set -euo pipefail

directory_path="$1"

if [[ ! -d "$directory_path" ]]; then
    exit 0
fi

find "$directory_path" -mindepth 1 -maxdepth 1 -type d | LC_ALL=C sort
"#;

    let output = ssh_client.run_script(script, &[remote_path.to_owned()])?;
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }

    output
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return Err(TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Remote directory listing for {remote_path} contained an empty path."),
                ));
            }

            Ok(trimmed.to_owned())
        })
        .collect()
}
