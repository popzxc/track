use crate::scripts::remote_path_helpers_shell;

/// Starts the uploaded launcher in the background for a prepared run
/// directory and worktree.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LaunchRemoteDispatchScript;

impl LaunchRemoteDispatchScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
RUN_DIR="$(expand_remote_path "$1")"
WORKTREE_PATH="$(expand_remote_path "$2")"

mkdir -p "$RUN_DIR"
LAUNCHER_PATH="$RUN_DIR/launch.sh"
chmod +x "$LAUNCHER_PATH"
nohup bash "$LAUNCHER_PATH" "$RUN_DIR" "$WORKTREE_PATH" >/dev/null 2>&1 </dev/null &
"#,
            path_helpers = remote_path_helpers_shell(),
        )
    }

    pub(crate) fn arguments(&self, remote_run_directory: &str, worktree_path: &str) -> Vec<String> {
        vec![remote_run_directory.to_owned(), worktree_path.to_owned()]
    }
}
