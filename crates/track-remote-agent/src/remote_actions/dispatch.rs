use track_types::errors::{ErrorCode, TrackError};
use track_types::remote_layout::{DispatchRunDirectory, DispatchWorktreePath};
use track_types::types::RemoteAgentPreferredTool;

use crate::helper::{
    CancelRunRequest, EmptyResponse, LaunchRunRequest, ReadRunSnapshotsRequest,
    ReadRunSnapshotsResponse, RunSnapshot,
};
use crate::ssh::SshClient;
use crate::{RemoteRunObservedStatus, RemoteRunSnapshotView};

/// Publishes the remote launcher and starts one agent run inside a prepared
/// run directory and worktree.
pub(crate) struct LaunchRemoteDispatchAction<'a> {
    ssh_client: &'a SshClient,
    remote_run_directory: &'a DispatchRunDirectory,
    worktree_path: &'a DispatchWorktreePath,
    preferred_tool: RemoteAgentPreferredTool,
}

impl<'a> LaunchRemoteDispatchAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        remote_run_directory: &'a DispatchRunDirectory,
        worktree_path: &'a DispatchWorktreePath,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Self {
        Self {
            ssh_client,
            remote_run_directory,
            worktree_path,
            preferred_tool,
        }
    }

    pub(crate) async fn execute(&self) -> Result<(), TrackError> {
        self.ssh_client
            .run_helper_json::<_, EmptyResponse>(
                "launch-run",
                &LaunchRunRequest {
                    run_directory: self.remote_run_directory.as_str(),
                    worktree_path: self.worktree_path.as_str(),
                    preferred_tool: self.preferred_tool,
                    shell_prelude: (!self.ssh_client.shell_prelude().trim().is_empty())
                        .then_some(self.ssh_client.shell_prelude()),
                },
            )
            .await?;

        Ok(())
    }
}

/// Requests that an already-started remote agent run stop consuming remote
/// resources and release its run directory as soon as possible.
pub(crate) struct CancelRemoteDispatchAction<'a> {
    ssh_client: &'a SshClient,
    remote_run_directory: &'a DispatchRunDirectory,
}

impl<'a> CancelRemoteDispatchAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        remote_run_directory: &'a DispatchRunDirectory,
    ) -> Self {
        Self {
            ssh_client,
            remote_run_directory,
        }
    }

    pub(crate) async fn execute(&self) -> Result<(), TrackError> {
        self.ssh_client
            .run_helper_json::<_, EmptyResponse>(
                "cancel-run",
                &CancelRunRequest {
                    run_directory: self.remote_run_directory.as_str(),
                },
            )
            .await?;
        Ok(())
    }
}

/// Reads the batched snapshot files for a set of remote runs so local
/// reconciliation can refresh dispatch state from remote truth.
pub(crate) struct ReadDispatchSnapshotsAction<'a> {
    ssh_client: &'a SshClient,
    run_directories: &'a [DispatchRunDirectory],
}

impl<'a> ReadDispatchSnapshotsAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        run_directories: &'a [DispatchRunDirectory],
    ) -> Self {
        Self {
            ssh_client,
            run_directories,
        }
    }

    pub(crate) async fn execute(&self) -> Result<Vec<RemoteRunSnapshotView>, TrackError> {
        if self.run_directories.is_empty() {
            return Ok(Vec::new());
        }

        let run_directories = self
            .run_directories
            .iter()
            .map(|run_directory| run_directory.as_str().to_owned())
            .collect::<Vec<_>>();
        let response = self
            .ssh_client
            .run_helper_json::<_, ReadRunSnapshotsResponse>(
                "read-run-snapshots",
                &ReadRunSnapshotsRequest {
                    run_directories: &run_directories,
                },
            )
            .await?;

        response
            .snapshots
            .into_iter()
            .map(remote_snapshot_view)
            .collect()
    }
}

fn remote_snapshot_view(snapshot: RunSnapshot) -> Result<RemoteRunSnapshotView, TrackError> {
    let run_directory = DispatchRunDirectory::new(&snapshot.run_directory).map_err(|error| {
        TrackError::new(
            ErrorCode::RemoteDispatchFailed,
            format!(
                "Remote helper returned an invalid run directory `{}` while reading dispatch snapshots: {error}. The remote workspace may be corrupted; reset the remote workspace and retry.",
                snapshot.run_directory
            ),
        )
    })?;

    Ok(RemoteRunSnapshotView {
        run_directory,
        status: RemoteRunObservedStatus::from_status_file_contents(snapshot.status),
        result: snapshot.result,
        stderr: snapshot.stderr,
        finished_at: snapshot.finished_at,
    })
}

#[cfg(test)]
mod tests {
    use crate::helper::RunSnapshot;
    use track_types::errors::ErrorCode;

    use super::remote_snapshot_view;

    #[test]
    fn rejects_invalid_remote_snapshot_run_directory() {
        let error = remote_snapshot_view(RunSnapshot {
            run_directory: "/tmp/not-a-dispatch-run".to_owned(),
            status: Some("running".to_owned()),
            result: None,
            stderr: None,
            finished_at: None,
        })
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::RemoteDispatchFailed);
        assert!(
            error
                .message()
                .contains("Remote helper returned an invalid run directory"),
            "unexpected error: {error}"
        );
    }
}
