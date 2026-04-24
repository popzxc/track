use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tempfile::TempDir;
use track_config::config::{RemoteAgentConfigFile, RemoteAgentReviewFollowUpConfigFile};
use track_types::types::RemoteAgentPreferredTool;

const FIXTURE_IMAGE: &str = "track-testing/ssh-fixture:local";
const FIXTURE_HOST: &str = "127.0.0.1";
const FIXTURE_USER: &str = "track";
const FIXTURE_WORKSPACE_ROOT: &str = "/home/track/workspace";
const FIXTURE_PROJECTS_REGISTRY_PATH: &str = "/srv/track-testing/state/track-projects.json";
const FIXTURE_SHELL_PRELUDE: &str =
    "export PATH=\"/opt/track-testing/bin:$PATH\"\nexport TRACK_TESTING_RUNTIME_DIR=\"/srv/track-testing\"";
const FIXTURE_START_ATTEMPTS: usize = 5;

// =============================================================================
// SSH Fixture Lifecycle
// =============================================================================
//
// The fixture controller owns container creation, key generation, and repo
// seeding so the Rust tests can talk in terms of behavior instead of raw Docker
// commands. Keeping that boundary here also makes it easier to reuse the same
// container contract later from browser end-to-end tests.
pub struct RemoteFixture {
    container_name: String,
    pub port: u16,
    // key_dir is kept separate from runtime_dir so the container entrypoint's
    // `chmod -R 777 $RUNTIME_DIR` does not make the private key world-readable,
    // which OpenSSH rejects with "bad permissions".
    key_dir: TempDir,
    runtime_dir: TempDir,
    workspace_root: PathBuf,
}

impl RemoteFixture {
    pub fn start(workspace_root: &Path) -> Self {
        let runtime_dir = TempDir::new().expect("fixture runtime tempdir should be created");
        let key_dir = TempDir::new().expect("key tempdir should be created");
        let container_name = format!(
            "track-testing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after the Unix epoch")
                .as_nanos()
        );
        run_fixturectl(workspace_root, ["build-image", "--image", FIXTURE_IMAGE]);
        run_fixturectl(
            workspace_root,
            [
                "generate-key",
                "--output-prefix",
                key_dir.path().join("id_ed25519").to_string_lossy().as_ref(),
            ],
        );
        let mut port = None;
        let mut last_fixture_output = None;
        for _attempt in 0..FIXTURE_START_ATTEMPTS {
            let candidate_port = reserve_local_port();
            let run_output = run_fixturectl_maybe(
                workspace_root,
                [
                    "run",
                    "--image",
                    FIXTURE_IMAGE,
                    "--name",
                    &container_name,
                    "--port",
                    &candidate_port.to_string(),
                    "--runtime-dir",
                    runtime_dir.path().to_string_lossy().as_ref(),
                    "--authorized-key",
                    key_dir
                        .path()
                        .join("id_ed25519.pub")
                        .to_string_lossy()
                        .as_ref(),
                ],
            )
            .expect("fixturectl run command should start successfully");
            if !run_output.status.success() {
                if fixture_port_bind_failed(&run_output) {
                    last_fixture_output = Some(run_output);
                    continue;
                }
                assert_command_success("fixturectl", &run_output);
            }

            let wait_output = run_fixturectl_maybe(
                workspace_root,
                [
                    "wait-for-ssh",
                    "--host",
                    FIXTURE_HOST,
                    "--user",
                    FIXTURE_USER,
                    "--port",
                    &candidate_port.to_string(),
                    "--private-key",
                    key_dir.path().join("id_ed25519").to_string_lossy().as_ref(),
                    "--known-hosts",
                    runtime_dir
                        .path()
                        .join("known_hosts")
                        .to_string_lossy()
                        .as_ref(),
                    "--timeout-seconds",
                    "20",
                ],
            )
            .expect("fixturectl wait-for-ssh command should start successfully");
            if wait_output.status.success() {
                port = Some(candidate_port);
                break;
            }

            let _ = run_fixturectl_maybe(workspace_root, ["stop", "--name", &container_name]);
            last_fixture_output = Some(wait_output);
        }
        let port = match port {
            Some(port) => port,
            None => {
                if let Some(output) = last_fixture_output.as_ref() {
                    assert_command_success("fixturectl", output);
                }
                panic!(
                    "fixturectl did not start the SSH fixture after {FIXTURE_START_ATTEMPTS} attempts"
                );
            }
        };

        Self {
            container_name,
            port,
            key_dir,
            runtime_dir,
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    pub fn seed_repo(&self, repo_url: &str, base_branch: &str, fork_owner: &str, login: &str) {
        run_fixturectl(
            &self.workspace_root,
            [
                "seed-repo",
                "--runtime-dir",
                self.runtime_dir.path().to_string_lossy().as_ref(),
                "--repo-url",
                repo_url,
                "--base-branch",
                base_branch,
                "--fork-owner",
                fork_owner,
                "--login",
                login,
            ],
        );
    }

    pub fn write_codex_state(&self, state: &Value) {
        fs::write(
            self.runtime_dir.path().join("state/codex.json"),
            serde_json::to_string_pretty(state).expect("codex state should serialize") + "\n",
        )
        .expect("codex state should be written");
    }

    pub fn write_claude_state(&self, state: &Value) {
        fs::write(
            self.runtime_dir.path().join("state/claude.json"),
            serde_json::to_string_pretty(state).expect("claude state should serialize") + "\n",
        )
        .expect("claude state should be written");
    }

    pub fn private_key_path(&self) -> PathBuf {
        self.key_dir.path().join("id_ed25519")
    }

    pub fn remote_agent_config(&self) -> RemoteAgentConfigFile {
        RemoteAgentConfigFile {
            host: FIXTURE_HOST.to_owned(),
            user: FIXTURE_USER.to_owned(),
            port: self.port,
            workspace_root: FIXTURE_WORKSPACE_ROOT.to_owned(),
            projects_registry_path: FIXTURE_PROJECTS_REGISTRY_PATH.to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: Some(FIXTURE_SHELL_PRELUDE.to_owned()),
            review_follow_up: Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: false,
                main_user: Some("octocat".to_owned()),
                default_review_prompt: Some(
                    "Focus on bugs, regressions, and missing tests.".to_owned(),
                ),
            }),
        }
    }

    pub fn read_remote_file(&self, remote_path: &str) -> String {
        let output = Command::new("ssh")
            .arg("-i")
            .arg(self.private_key_path())
            .arg("-p")
            .arg(self.port.to_string())
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
            ])
            .arg(format!(
                "UserKnownHostsFile={}",
                self.runtime_dir
                    .path()
                    .join("known_hosts")
                    .to_string_lossy()
            ))
            .arg(format!("{FIXTURE_USER}@{FIXTURE_HOST}"))
            .arg("cat")
            .arg(remote_path)
            .output()
            .expect("remote file read command should start");

        if output.status.success() {
            return String::from_utf8(output.stdout).expect("remote file contents should be UTF-8");
        }

        panic!(
            "failed to read remote file {remote_path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    pub fn read_log_entries(&self, log_name: &str) -> Vec<Value> {
        let log_path = self
            .runtime_dir
            .path()
            .join("logs")
            .join(format!("{log_name}.jsonl"));
        let Ok(contents) = fs::read_to_string(&log_path) else {
            return Vec::new();
        };

        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("fixture log line should be valid JSON"))
            .collect()
    }

    pub fn remote_path_exists(&self, remote_path: &str) -> bool {
        let output = Command::new("ssh")
            .arg("-i")
            .arg(self.private_key_path())
            .arg("-p")
            .arg(self.port.to_string())
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
            ])
            .arg(format!(
                "UserKnownHostsFile={}",
                self.runtime_dir
                    .path()
                    .join("known_hosts")
                    .to_string_lossy()
            ))
            .arg(format!("{FIXTURE_USER}@{FIXTURE_HOST}"))
            .arg("test")
            .arg("-e")
            .arg(remote_path)
            .output()
            .expect("remote path existence command should start");

        output.status.success()
    }
}

impl Drop for RemoteFixture {
    fn drop(&mut self) {
        let _ = run_fixturectl_maybe(
            &self.workspace_root,
            ["stop", "--name", &self.container_name],
        );
    }
}

fn reserve_local_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("port reservation listener should bind")
        .local_addr()
        .expect("reserved port should have a local address")
        .port()
}

fn run_fixturectl<const N: usize>(workspace_root: &Path, args: [&str; N]) {
    let output = run_fixturectl_maybe(workspace_root, args)
        .expect("fixturectl command should start successfully");
    assert_command_success("fixturectl", &output);
}

fn run_fixturectl_maybe<const N: usize>(
    workspace_root: &Path,
    args: [&str; N],
) -> Result<Output, std::io::Error> {
    Command::new("python3")
        .arg(workspace_root.join("testing/support/fixturectl.py"))
        .args(args)
        .current_dir(workspace_root)
        .output()
}

fn assert_command_success(label: &str, output: &Output) {
    if output.status.success() {
        return;
    }

    panic!(
        "{label} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture_port_bind_failed(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr).contains("failed to bind host port")
}
