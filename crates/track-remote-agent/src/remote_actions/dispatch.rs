use track_types::errors::TrackError;
use track_types::types::RemoteAgentPreferredTool;

use crate::remote_actions::UploadRemoteFileAction;
use crate::scripts::{
    CancelRemoteDispatchScript, LaunchRemoteDispatchScript, ReadDispatchSnapshotsScript,
    RemoteAgentLauncherScript,
};
use crate::ssh::SshClient;
use crate::types::RemoteDispatchSnapshot;

/// Publishes the remote launcher and starts one agent run inside a prepared
/// run directory and worktree.
pub(crate) struct LaunchRemoteDispatchAction<'a> {
    ssh_client: &'a SshClient,
    remote_run_directory: &'a str,
    worktree_path: &'a str,
    preferred_tool: RemoteAgentPreferredTool,
}

impl<'a> LaunchRemoteDispatchAction<'a> {
    pub(crate) fn new(
        ssh_client: &'a SshClient,
        remote_run_directory: &'a str,
        worktree_path: &'a str,
        preferred_tool: RemoteAgentPreferredTool,
    ) -> Self {
        Self {
            ssh_client,
            remote_run_directory,
            worktree_path,
            preferred_tool,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let launcher_contents =
            RemoteAgentLauncherScript::new(self.preferred_tool, self.ssh_client.shell_prelude())
                .render();
        UploadRemoteFileAction::new(
            self.ssh_client,
            &format!("{}/launch.sh", self.remote_run_directory),
            &launcher_contents,
        )
        .execute()?;

        let script = LaunchRemoteDispatchScript;
        let arguments = script.arguments(self.remote_run_directory, self.worktree_path);
        self.ssh_client.run_script(&script.render(), &arguments)?;

        Ok(())
    }
}

/// Requests that an already-started remote agent run stop consuming remote
/// resources and release its run directory as soon as possible.
pub(crate) struct CancelRemoteDispatchAction<'a> {
    ssh_client: &'a SshClient,
    remote_run_directory: &'a str,
}

impl<'a> CancelRemoteDispatchAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, remote_run_directory: &'a str) -> Self {
        Self {
            ssh_client,
            remote_run_directory,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), TrackError> {
        let script = CancelRemoteDispatchScript;
        let arguments = script.arguments(self.remote_run_directory);
        self.ssh_client.run_script(&script.render(), &arguments)?;
        Ok(())
    }
}

/// Reads the batched snapshot files for a set of remote runs so local
/// reconciliation can refresh dispatch state from remote truth.
pub(crate) struct ReadDispatchSnapshotsAction<'a> {
    ssh_client: &'a SshClient,
    run_directories: &'a [String],
}

impl<'a> ReadDispatchSnapshotsAction<'a> {
    pub(crate) fn new(ssh_client: &'a SshClient, run_directories: &'a [String]) -> Self {
        Self {
            ssh_client,
            run_directories,
        }
    }

    pub(crate) fn execute(&self) -> Result<Vec<RemoteDispatchSnapshot>, TrackError> {
        if self.run_directories.is_empty() {
            return Ok(Vec::new());
        }

        let script = ReadDispatchSnapshotsScript;
        let report = self
            .ssh_client
            .run_script(&script.render(), self.run_directories)?;

        script.parse_report(&report)
    }
}
