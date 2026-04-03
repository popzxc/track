use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_STATUS_FILE_NAME,
};
use crate::scripts::remote_path_helpers_shell;

/// Cancels an active remote run by killing the launcher and model processes and
/// marking the run as canceled.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CancelRemoteDispatchScript;

impl CancelRemoteDispatchScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
RUN_DIR="$(expand_remote_path "$1")"
LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"
STATUS_FILE="$RUN_DIR/{status_file}"
FINISHED_AT_FILE="$RUN_DIR/{finished_at_file}"

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

if [ -f "$LAUNCHER_PID_FILE" ]; then
  LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
  kill_if_running "$LAUNCHER_PID"
fi

if [ -f "$CODEX_PID_FILE" ]; then
  CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
  kill_if_running "$CODEX_PID"
fi

mkdir -p "$RUN_DIR"
printf 'canceled\n' > "$STATUS_FILE"
date -u +%Y-%m-%dT%H:%M:%SZ > "$FINISHED_AT_FILE"
"#,
            path_helpers = remote_path_helpers_shell(),
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
            status_file = REMOTE_STATUS_FILE_NAME,
            finished_at_file = REMOTE_FINISHED_AT_FILE_NAME,
        )
    }

    pub(crate) fn arguments(&self, remote_run_directory: &str) -> Vec<String> {
        vec![remote_run_directory.to_owned()]
    }
}
