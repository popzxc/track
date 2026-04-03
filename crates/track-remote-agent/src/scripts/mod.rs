//! Remote shell-script definitions used by the remote-agent crate.
//!
//! Each script is represented as its own type so the service layer can talk in
//! terms of named remote operations instead of opaque shell snippets.

mod checkout;
mod cleanup;
mod dispatch;
mod files;

pub(crate) use checkout::{
    CreateReviewWorktreeScript, CreateWorktreeScript, EnsureCheckoutScript,
    EnsureFollowUpWorktreeScript,
};
pub(crate) use cleanup::{
    CleanupOrphanedRemoteArtifactsScript, CleanupReviewArtifactsScript,
    CleanupReviewWorkspaceCachesScript, CleanupTaskArtifactsScript, ResetWorkspaceScript,
};
pub(crate) use dispatch::{
    CancelRemoteDispatchScript, FetchGithubApiScript, FetchGithubLoginScript,
    LaunchRemoteDispatchScript, PostPullRequestCommentScript, ReadDispatchSnapshotsScript,
    RemoteAgentLauncherScript,
};
pub(crate) use files::{PrepareRemoteUploadScript, ReadRemoteFileScript};

/// Returns the shared shell function used by remote scripts to expand
/// home-relative paths in a predictable, non-interactive way.
///
/// Remote automation cannot rely on shell-specific tilde expansion rules once
/// paths start flowing through quoted variables and script arguments. This
/// helper gives every script the same stable path contract.
pub(crate) fn remote_path_helpers_shell() -> &'static str {
    r#"
expand_remote_path() {
  case "$1" in
    "~")
      printf '%s\n' "$HOME"
      ;;
    "~/"*)
      # Strip the literal `~/` prefix before joining with $HOME. Bash expands
      # an unescaped `~` inside `${var#pattern}` to the current home path,
      # which leaves the original `~/...` intact and produces `$HOME/~/...`.
      printf '%s/%s\n' "$HOME" "${1#\~/}"
      ;;
    *)
      printf '%s\n' "$1"
      ;;
  esac
}
"#
}

pub(crate) fn render_remote_script_with_shell_prelude(script: &str, shell_prelude: &str) -> String {
    let mut rendered = String::from("set -e\n");

    if !shell_prelude.trim().is_empty() {
        rendered.push_str(shell_prelude);
        if !shell_prelude.ends_with('\n') {
            rendered.push('\n');
        }
    }

    rendered.push('\n');
    rendered.push_str(script.trim_start_matches('\n'));
    rendered
}

#[cfg(test)]
mod tests {
    #[test]
    fn prepends_shell_prelude_before_remote_script_body() {
        let rendered = super::render_remote_script_with_shell_prelude(
            "set -eu\nprintf '%s\\n' done\n",
            "export NVM_DIR=\"$HOME/.nvm\"\n. \"$HOME/.cargo/env\"\n",
        );

        assert!(rendered.starts_with("set -e\n"));
        assert!(rendered.contains("export NVM_DIR=\"$HOME/.nvm\""));
        assert!(rendered.contains(". \"$HOME/.cargo/env\""));
        assert!(rendered.contains("printf '%s\\n' done"));
    }

    #[test]
    fn expands_tilde_prefixed_remote_paths_without_reintroducing_a_literal_tilde_segment() {
        let helper_script = format!(
            r#"
set -eu
HOME="/tmp/remote-home"
{path_helpers}
expand_remote_path "$1"
"#,
            path_helpers = super::remote_path_helpers_shell(),
        );

        let output = std::process::Command::new("bash")
            .arg("-s")
            .arg("--")
            .arg("~/workspace/project-a/dispatches/dispatch-1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;

                let stdin = child.stdin.as_mut().expect("bash stdin should exist");
                stdin
                    .write_all(helper_script.as_bytes())
                    .expect("helper script should write to bash stdin");
                child.wait_with_output()
            })
            .expect("bash helper should run");

        assert!(
            output.status.success(),
            "helper script should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "/tmp/remote-home/workspace/project-a/dispatches/dispatch-1"
        );
    }
}
