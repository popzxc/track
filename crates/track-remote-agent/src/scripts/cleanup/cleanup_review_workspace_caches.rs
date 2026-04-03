use crate::scripts::remote_path_helpers_shell;

/// Removes cached review checkout directories that are no longer needed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CleanupReviewWorkspaceCachesScript;

impl CleanupReviewWorkspaceCachesScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}

for RAW_CHECKOUT_PATH in "$@"; do
  CHECKOUT_PATH="$(expand_remote_path "$RAW_CHECKOUT_PATH")"
  WORKSPACE_PATH="$(dirname "$CHECKOUT_PATH")"

  if [ -d "$CHECKOUT_PATH/.git" ]; then
    git -C "$CHECKOUT_PATH" worktree prune >&2 || true
  fi

  if [ -e "$CHECKOUT_PATH" ]; then
    rm -rf "$CHECKOUT_PATH"
  fi

  if [ -d "$WORKSPACE_PATH" ]; then
    rmdir "$WORKSPACE_PATH" 2>/dev/null || true
  fi
done
"#,
            path_helpers = remote_path_helpers_shell(),
        )
    }
}
