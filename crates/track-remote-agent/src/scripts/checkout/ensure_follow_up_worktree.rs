use crate::scripts::remote_path_helpers_shell;

/// Reuses the existing branch worktree for a follow-up dispatch when possible.
///
/// Follow-up runs should keep working in the same branch context as the
/// original dispatch, so this script restores that worktree instead of creating
/// a brand-new branch from upstream.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EnsureFollowUpWorktreeScript;

impl EnsureFollowUpWorktreeScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
BRANCH_NAME="$2"
WORKTREE_PATH="$(expand_remote_path "$3")"

mkdir -p "$(dirname "$WORKTREE_PATH")"
git -C "$CHECKOUT_PATH" fetch origin --prune >&2 || true
git -C "$CHECKOUT_PATH" fetch upstream --prune >&2 || true

if [ -e "$WORKTREE_PATH/.git" ]; then
  if ! git -C "$WORKTREE_PATH" rev-parse --show-toplevel >/dev/null 2>&1; then
    echo "Existing follow-up worktree path $WORKTREE_PATH is not a valid Git worktree." >&2
    exit 1
  fi

  git -C "$WORKTREE_PATH" checkout "$BRANCH_NAME" >&2
  exit 0
fi

if [ -e "$WORKTREE_PATH" ]; then
  echo "Follow-up worktree path $WORKTREE_PATH already exists but is not a Git worktree." >&2
  exit 1
fi

git -C "$CHECKOUT_PATH" worktree prune >&2

if git -C "$CHECKOUT_PATH" show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
  git -C "$CHECKOUT_PATH" worktree add "$WORKTREE_PATH" "$BRANCH_NAME" >&2
  exit 0
fi

if git -C "$CHECKOUT_PATH" show-ref --verify --quiet "refs/remotes/origin/$BRANCH_NAME"; then
  git -C "$CHECKOUT_PATH" worktree add -B "$BRANCH_NAME" "$WORKTREE_PATH" "origin/$BRANCH_NAME" >&2
  exit 0
fi

echo "Could not restore the follow-up worktree for branch $BRANCH_NAME." >&2
exit 1
"#,
            path_helpers = remote_path_helpers_shell(),
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Vec<String> {
        vec![
            checkout_path.to_owned(),
            branch_name.to_owned(),
            worktree_path.to_owned(),
        ]
    }
}
