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
