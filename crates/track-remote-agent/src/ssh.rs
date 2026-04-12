use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use track_config::paths::{collapse_home_path, path_to_string};
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};

use crate::scripts::render_remote_script_with_shell_prelude;

pub(crate) struct SshClient {
    host: String,
    key_path: PathBuf,
    known_hosts_path: PathBuf,
    port: u16,
    shell_prelude: String,
    user: String,
}

pub(crate) enum ScriptOutput {
    Success(String),
    ExitCode(i32),
    Failure(String),
}

impl SshClient {
    pub(crate) fn new(config: &RemoteAgentRuntimeConfig) -> Result<Self, TrackError> {
        if !config.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before cleaning task.",
                    collapse_home_path(&config.managed_key_path)
                ),
            ));
        }

        if let Some(parent_directory) = config.managed_known_hosts_path.parent() {
            fs::create_dir_all(parent_directory).map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "Could not create the managed known_hosts directory at {}: {error}",
                        collapse_home_path(parent_directory)
                    ),
                )
            })?;
        }

        if !config.managed_known_hosts_path.exists() {
            fs::write(&config.managed_known_hosts_path, "").map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!(
                        "Could not create the managed known_hosts file at {}: {error}",
                        collapse_home_path(&config.managed_known_hosts_path)
                    ),
                )
            })?;
        }

        Ok(Self {
            host: config.host.clone(),
            key_path: config.managed_key_path.clone(),
            known_hosts_path: config.managed_known_hosts_path.clone(),
            port: config.port,
            shell_prelude: config.shell_prelude.clone().unwrap_or_default(),
            user: config.user.clone(),
        })
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    pub(crate) fn shell_prelude(&self) -> &str {
        &self.shell_prelude
    }

    pub(crate) fn run_script(&self, script: &str, args: &[String]) -> Result<String, TrackError> {
        match self.run_script_with_exit_code(script, args)? {
            ScriptOutput::Success(stdout) => Ok(stdout),
            ScriptOutput::ExitCode(code) => Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Remote command exited with unexpected status code {code}."),
            )),
            ScriptOutput::Failure(stderr) => {
                Err(TrackError::new(ErrorCode::RemoteDispatchFailed, stderr))
            }
        }
    }

    pub(crate) fn run_script_with_exit_code(
        &self,
        script: &str,
        args: &[String],
    ) -> Result<ScriptOutput, TrackError> {
        let mut command = self.base_ssh_command();
        command.arg(format!("{}@{}", self.user, self.host));
        command.arg("bash");
        command.arg("-s");
        command.arg("--");
        command.args(args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not start the remote SSH command: {error}"),
            )
        })?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not open stdin for the remote SSH command.",
            ));
        };
        let rendered_script = render_remote_script_with_shell_prelude(script, &self.shell_prelude);
        stdin
            .write_all(rendered_script.as_bytes())
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Could not write the remote shell script to SSH stdin: {error}"),
                )
            })?;
        drop(stdin);

        let output = child.wait_with_output().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not wait for the remote SSH command to finish: {error}"),
            )
        })?;

        if output.status.success() {
            return Ok(ScriptOutput::Success(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            ));
        }

        let exit_code = output.status.code();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if let Some(exit_code) = exit_code {
            if stderr.is_empty() {
                return Ok(ScriptOutput::ExitCode(exit_code));
            }

            if exit_code == 3 {
                return Ok(ScriptOutput::ExitCode(exit_code));
            }
        }

        Ok(ScriptOutput::Failure(if stderr.is_empty() {
            "Remote command failed without stderr output.".to_owned()
        } else {
            stderr
        }))
    }

    pub(crate) fn copy_local_file_to_remote(
        &self,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<(), TrackError> {
        let output = self
            .base_scp_command()
            .arg(local_path)
            .arg(format!("{}@{}:{remote_path}", self.user, self.host))
            .output()
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::RemoteDispatchFailed,
                    format!("Could not start `scp` for remote dispatch: {error}"),
                )
            })?;

        if !output.status.success() {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Could not upload the remote file at {remote_path}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }

        Ok(())
    }

    fn base_ssh_command(&self) -> Command {
        let mut command = Command::new("ssh");
        command.arg("-i");
        command.arg(&self.key_path);
        command.arg("-p");
        command.arg(self.port.to_string());
        command.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
        ]);
        command.arg(format!(
            "UserKnownHostsFile={}",
            path_to_string(&self.known_hosts_path)
        ));
        command
    }

    fn base_scp_command(&self) -> Command {
        let mut command = Command::new("scp");
        command.arg("-i");
        command.arg(&self.key_path);
        command.arg("-P");
        command.arg(self.port.to_string());
        command.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
        ]);
        command.arg(format!(
            "UserKnownHostsFile={}",
            path_to_string(&self.known_hosts_path)
        ));
        command
    }
}
