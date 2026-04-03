use crate::scripts::remote_path_helpers_shell;

/// Creates a fresh task worktree from the project's upstream base branch.
///
/// Task dispatches are expected to start from a clean branch rooted at the
/// current upstream base branch, so this script recreates the worktree when
/// necessary instead of trying to repair unknown local state.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CreateWorktreeScript;

impl CreateWorktreeScript {
    pub(crate) fn render(&self) -> String {
        format!(
            r#"
set -eu
{path_helpers}
CHECKOUT_PATH="$(expand_remote_path "$1")"
BASE_BRANCH="$2"
BRANCH_NAME="$3"
WORKTREE_PATH="$(expand_remote_path "$4")"

mkdir -p "$(dirname "$WORKTREE_PATH")"

worktree_is_registered() {{
  git -C "$CHECKOUT_PATH" worktree list --porcelain | grep -F "worktree $WORKTREE_PATH" >/dev/null 2>&1
}}

if [ -e "$WORKTREE_PATH" ]; then
  if worktree_is_registered; then
    git -C "$CHECKOUT_PATH" worktree remove --force "$WORKTREE_PATH" >&2 || true
  else
    echo "Refusing to overwrite unexpected existing path at $WORKTREE_PATH while preparing a fresh dispatch worktree." >&2
    exit 1
  fi
fi

git -C "$CHECKOUT_PATH" worktree prune >&2
git -C "$CHECKOUT_PATH" worktree add -B "$BRANCH_NAME" "$WORKTREE_PATH" "upstream/$BASE_BRANCH" >&2
"#,
            path_helpers = remote_path_helpers_shell(),
        )
    }

    pub(crate) fn arguments(
        &self,
        checkout_path: &str,
        base_branch: &str,
        branch_name: &str,
        worktree_path: &str,
    ) -> Vec<String> {
        vec![
            checkout_path.to_owned(),
            base_branch.to_owned(),
            branch_name.to_owned(),
            worktree_path.to_owned(),
        ]
    }
}
