use track_types::errors::TrackError;

use crate::constants::{
    REMOTE_CODEX_PID_FILE_NAME, REMOTE_FINISHED_AT_FILE_NAME, REMOTE_LAUNCHER_PID_FILE_NAME,
    REMOTE_STATUS_FILE_NAME,
};
use crate::scripts::remote_path_helpers_shell;
use crate::types::{RemoteArtifactCleanupCounts, RemoteTaskCleanupMode};

use super::parse_remote_cleanup_counts;

/// Removes task-owned remote worktrees and, when appropriate, their run
/// directories.
///
/// Closing and deleting a task have different retention semantics, so this
/// script accepts an explicit cleanup mode rather than inferring the desired
/// behavior from the caller.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CleanupTaskArtifactsScript {
    cleanup_remote_dispatch_directories: bool,
}

impl CleanupTaskArtifactsScript {
    pub(crate) fn from_mode(cleanup_mode: RemoteTaskCleanupMode) -> Self {
        Self {
            cleanup_remote_dispatch_directories: cleanup_mode == RemoteTaskCleanupMode::DeleteTask,
        }
    }

    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
shift

WORKTREE_PATHS=()
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--" ]; then
    shift
    break
  fi

  WORKTREE_PATHS+=("$1")
  shift
done

RUN_DIRECTORIES=("$@")
WORKTREES_REMOVED=0
RUN_DIRECTORIES_REMOVED=0

kill_if_running() {{
  PID="$1"
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null || true
  fi
}}

worktree_is_registered() {{
  TARGET_WORKTREE="$1"
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $TARGET_WORKTREE" >/dev/null 2>&1
}}

for RAW_RUN_DIR in "${{RUN_DIRECTORIES[@]}}"; do
  RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
  LAUNCHER_PID_FILE="$RUN_DIR/{launcher_pid_file}"
  CODEX_PID_FILE="$RUN_DIR/{codex_pid_file}"
  STATUS_FILE="$RUN_DIR/{status_file}"
  FINISHED_AT_FILE="$RUN_DIR/{finished_at_file}"
  CURRENT_STATUS="$(tr -d '[:space:]' < "$STATUS_FILE" 2>/dev/null || true)"

  if [ -f "$LAUNCHER_PID_FILE" ]; then
    LAUNCHER_PID="$(tr -d '[:space:]' < "$LAUNCHER_PID_FILE")"
    kill_if_running "$LAUNCHER_PID"
  fi

  if [ -f "$CODEX_PID_FILE" ]; then
    CODEX_PID="$(tr -d '[:space:]' < "$CODEX_PID_FILE")"
    kill_if_running "$CODEX_PID"
  fi

  if [ -d "$RUN_DIR" ] && {{ [ "$CURRENT_STATUS" = "preparing" ] || [ "$CURRENT_STATUS" = "running" ]; }}; then
    printf 'canceled\n' > "$STATUS_FILE"
    date -u +%Y-%m-%dT%H:%M:%SZ > "$FINISHED_AT_FILE"
  fi
done

for RAW_WORKTREE_PATH in "${{WORKTREE_PATHS[@]}}"; do
  WORKTREE_PATH="$(expand_remote_path "$RAW_WORKTREE_PATH")"
  HAD_WORKTREE_PATH="false"
  if [ -e "$WORKTREE_PATH" ]; then
    HAD_WORKTREE_PATH="true"
  fi

  if [ -d "$CHECKOUT_PATH/.git" ] && worktree_is_registered "$WORKTREE_PATH"; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  fi

  if [ -e "$WORKTREE_PATH" ]; then
    rm -rf "$WORKTREE_PATH"
  fi

  if [ "$HAD_WORKTREE_PATH" = "true" ] && [ ! -e "$WORKTREE_PATH" ]; then
    WORKTREES_REMOVED=$((WORKTREES_REMOVED + 1))
  fi
done

if [ -d "$CHECKOUT_PATH/.git" ]; then
  git -C "$CHECKOUT_PATH" worktree prune >&2 || true
fi

if [ "{cleanup_remote_dispatch_directories}" = "true" ]; then
  for RAW_RUN_DIR in "${{RUN_DIRECTORIES[@]}}"; do
    RUN_DIR="$(expand_remote_path "$RAW_RUN_DIR")"
    HAD_RUN_DIRECTORY="false"
    if [ -e "$RUN_DIR" ]; then
      HAD_RUN_DIRECTORY="true"
    fi
    if [ -e "$RUN_DIR" ]; then
      rm -rf "$RUN_DIR"
    fi
    if [ "$HAD_RUN_DIRECTORY" = "true" ] && [ ! -e "$RUN_DIR" ]; then
      RUN_DIRECTORIES_REMOVED=$((RUN_DIRECTORIES_REMOVED + 1))
    fi
  done
fi

printf '{{"worktreesRemoved":%s,"runDirectoriesRemoved":%s}}\n' \
  "$WORKTREES_REMOVED" \
  "$RUN_DIRECTORIES_REMOVED"
"#,
            path_helpers = remote_path_helpers_shell(),
            cleanup_remote_dispatch_directories = if self.cleanup_remote_dispatch_directories {
                "true"
            } else {
                "false"
            },
            launcher_pid_file = REMOTE_LAUNCHER_PID_FILE_NAME,
            codex_pid_file = REMOTE_CODEX_PID_FILE_NAME,
            status_file = REMOTE_STATUS_FILE_NAME,
            finished_at_file = REMOTE_FINISHED_AT_FILE_NAME,
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &str,
        worktree_paths: &[String],
        run_directories: &[String],
    ) -> Vec<String> {
        let mut arguments = vec![checkout_path.to_owned()];
        arguments.extend(worktree_paths.iter().cloned());
        arguments.push("--".to_owned());
        arguments.extend(run_directories.iter().cloned());
        arguments
    }

    pub(crate) fn parse_report(
        &self,
        report: &str,
    ) -> Result<RemoteArtifactCleanupCounts, TrackError> {
        parse_remote_cleanup_counts(report)
    }
}
