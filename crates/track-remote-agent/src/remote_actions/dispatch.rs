use track_types::errors::TrackError;
use track_types::remote_layout::{DispatchRunDirectory, DispatchWorktreePath};
use track_types::types::RemoteAgentPreferredTool;

use crate::helper::{
    CancelRunRequest, EmptyResponse, LaunchRunRequest, ReadRunSnapshotsRequest,
    ReadRunSnapshotsResponse,
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

        Ok(response
            .snapshots
            .into_iter()
            .map(|snapshot| RemoteRunSnapshotView {
                run_directory: DispatchRunDirectory::from_db_unchecked(snapshot.run_directory),
                status: RemoteRunObservedStatus::from_status_file_contents(snapshot.status),
                result: snapshot.result,
                stderr: snapshot.stderr,
                finished_at: snapshot.finished_at,
            })
            .collect())
    }
}
