use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use serde::de::DeserializeOwned;
use serde::Serialize;
use track_config::paths::{collapse_home_path, path_to_string};
use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::{ErrorCode, TrackError};

use crate::scripts::{remote_path_helpers_shell, render_remote_script_with_shell_prelude};

const REMOTE_HELPER_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/track-remote-helper.pyz"));
const REMOTE_HELPER_VERSION: &str = env!("TRACK_REMOTE_HELPER_VERSION");
const REMOTE_HELPER_DIRECTORY_NAME: &str = ".track-remote-helper";
const REMOTE_HELPER_FILE_NAME: &str = "track-remote-helper.pyz";
const REMOTE_HELPER_VERSION_FILE_NAME: &str = "version.txt";

pub(crate) struct SshClient {
    helper_directory: String,
    helper_path: String,
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

        let helper_directory = format!(
            "{}/{}",
            config.workspace_root.trim_end_matches('/'),
            REMOTE_HELPER_DIRECTORY_NAME
        );

        Ok(Self {
            helper_path: format!("{helper_directory}/{REMOTE_HELPER_FILE_NAME}"),
            helper_directory,
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

    pub(crate) fn run_helper_json<Request, Response>(
        &self,
        command_name: &str,
        request: &Request,
    ) -> Result<Response, TrackError>
    where
        Request: Serialize,
        Response: DeserializeOwned,
    {
        self.ensure_remote_helper_uploaded()?;

        let request_json = serde_json::to_vec(request).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not serialize the remote helper request: {error}"),
            )
        })?;

        let mut command = self.base_ssh_command();
        command.arg(format!("{}@{}", self.user, self.host));
        command.arg(format!(
            "bash -lc {}",
            shell_single_quote(&self.remote_helper_bootstrap_command(command_name))
        ));
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not start the remote helper command: {error}"),
            )
        })?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not open stdin for the remote helper command.",
            ));
        };
        stdin.write_all(&request_json).map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not write the remote helper request body: {error}"),
            )
        })?;
        drop(stdin);

        let output = child.wait_with_output().map_err(|error| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!("Could not wait for the remote helper command to finish: {error}"),
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                if stderr.is_empty() {
                    "Remote helper command failed without stderr output.".to_owned()
                } else {
                    stderr
                },
            ));
        }

        serde_json::from_slice::<Response>(&output.stdout).map_err(|error| {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stderr_suffix = if stderr.is_empty() {
                String::new()
            } else {
                format!(" Stderr: {stderr}")
            };
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Remote helper command `{command_name}` returned invalid JSON: {error}. Raw stdout: {stdout}.{stderr_suffix}"
                ),
            )
        })
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

    fn ensure_remote_helper_uploaded(&self) -> Result<(), TrackError> {
        let mut helper_uploaded = helper_upload_state()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *helper_uploaded {
            return Ok(());
        }

        if self.remote_helper_is_current()? {
            *helper_uploaded = true;
            return Ok(());
        }

        self.upload_remote_helper()?;
        *helper_uploaded = true;
        Ok(())
    }

    fn remote_helper_is_current(&self) -> Result<bool, TrackError> {
        let script = format!(
            r#"
set -eu
{path_helpers}
HELPER_DIRECTORY="$(expand_remote_path "$1")"
HELPER_PATH="$HELPER_DIRECTORY/{helper_file}"
VERSION_PATH="$HELPER_DIRECTORY/{version_file}"

if [ -f "$HELPER_PATH" ] && [ -f "$VERSION_PATH" ]; then
  CURRENT_VERSION="$(tr -d '[:space:]' < "$VERSION_PATH")"
  if [ "$CURRENT_VERSION" = "$2" ]; then
    printf 'current\n'
    exit 0
  fi
fi

mkdir -p "$HELPER_DIRECTORY"
printf 'stale\n'
"#,
            path_helpers = remote_path_helpers_shell(),
            helper_file = REMOTE_HELPER_FILE_NAME,
            version_file = REMOTE_HELPER_VERSION_FILE_NAME,
        );
        let output = self.run_script(
            &script,
            &[
                self.helper_directory.clone(),
                REMOTE_HELPER_VERSION.to_owned(),
            ],
        )?;
        Ok(output.trim() == "current")
    }

    fn upload_remote_helper(&self) -> Result<(), TrackError> {
        let local_temp_file =
            std::env::temp_dir().join(format!("track-remote-helper-{}.pyz", std::process::id()));
        fs::write(&local_temp_file, REMOTE_HELPER_BYTES).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not write the packaged remote helper to {}: {error}",
                    path_to_string(&local_temp_file)
                ),
            )
        })?;

        let upload_result = self.copy_local_file_to_remote(&local_temp_file, &self.helper_path);
        let _ = fs::remove_file(&local_temp_file);
        upload_result?;

        let script = format!(
            r#"
set -eu
{path_helpers}
VERSION_PATH="$(expand_remote_path "$1")"
mkdir -p "$(dirname "$VERSION_PATH")"
printf '%s\n' "$2" > "$VERSION_PATH"
"#,
            path_helpers = remote_path_helpers_shell(),
        );
        self.run_script(
            &script,
            &[
                format!(
                    "{}/{}",
                    self.helper_directory, REMOTE_HELPER_VERSION_FILE_NAME
                ),
                REMOTE_HELPER_VERSION.to_owned(),
            ],
        )?;

        Ok(())
    }

    fn remote_helper_bootstrap_command(&self, command_name: &str) -> String {
        let mut command = String::from("set -e\n");
        if !self.shell_prelude.trim().is_empty() {
            command.push_str(&self.shell_prelude);
            if !self.shell_prelude.ends_with('\n') {
                command.push('\n');
            }
        }
        command.push_str(&format!(
            r#"expand_remote_path() {{
  case "$1" in
    "~")
      printf '%s\n' "$HOME"
      ;;
    "~/"*)
      printf '%s/%s\n' "$HOME" "${{1#\~/}}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}}
HELPER_PATH="$(expand_remote_path {helper_path})"
export TRACK_REMOTE_HELPER_SELF="$HELPER_PATH"
exec python3 -B "$HELPER_PATH" {command_name}
"#,
            helper_path = shell_single_quote(&self.helper_path),
            command_name = shell_single_quote(command_name),
        ));
        command
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

fn helper_upload_state() -> &'static Mutex<bool> {
    static HELPER_UPLOADED: OnceLock<Mutex<bool>> = OnceLock::new();
    HELPER_UPLOADED.get_or_init(|| Mutex::new(false))
}

pub fn invalidate_helper_upload() {
    let mut helper_uploaded = helper_upload_state().lock().unwrap();
    *helper_uploaded = false;
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}
