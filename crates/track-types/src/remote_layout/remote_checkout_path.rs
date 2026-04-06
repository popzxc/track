use serde::{Deserialize, Serialize};

use crate::ids::DispatchId;

use super::{
    impl_string_value, DispatchRunDirectory, DispatchWorktreePath, WorkspaceKey,
    REVIEW_RUN_DIRECTORY_NAME, REVIEW_WORKTREE_DIRECTORY_NAME, TASK_RUN_DIRECTORY_NAME,
    TASK_WORKTREE_DIRECTORY_NAME,
};

/// Absolute remote path to the long-lived repository checkout that acts as the
/// source checkout for derived dispatch worktrees and run directories.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemoteCheckoutPath(String);

impl RemoteCheckoutPath {
    pub fn for_workspace(workspace_root: &str, workspace_key: &WorkspaceKey) -> Self {
        let workspace_key = workspace_key.clone().into_inner();
        Self(format!(
            "{}/{}/{}",
            workspace_root.trim_end_matches('/'),
            workspace_key,
            workspace_key
        ))
    }

    pub fn from_registry_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn task_worktree(&self, dispatch_id: &DispatchId) -> DispatchWorktreePath {
        DispatchWorktreePath::from_layout(format!(
            "{}/{}/{}",
            self.workspace_directory(),
            TASK_WORKTREE_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn review_worktree(&self, dispatch_id: &DispatchId) -> DispatchWorktreePath {
        DispatchWorktreePath::from_layout(format!(
            "{}/{}/{}",
            self.workspace_directory(),
            REVIEW_WORKTREE_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn task_run_directory(&self, dispatch_id: &DispatchId) -> DispatchRunDirectory {
        DispatchRunDirectory::from_layout(format!(
            "{}/{}/{}",
            self.workspace_directory(),
            TASK_RUN_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn review_run_directory(&self, dispatch_id: &DispatchId) -> DispatchRunDirectory {
        DispatchRunDirectory::from_layout(format!(
            "{}/{}/{}",
            self.workspace_directory(),
            REVIEW_RUN_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    fn workspace_directory(&self) -> &str {
        self.0
            .rsplit_once('/')
            .map(|(prefix, _leaf)| prefix)
            .expect("checkout paths should include a workspace directory")
    }
}

impl_string_value!(RemoteCheckoutPath);

#[cfg(test)]
mod tests {
    use crate::ids::DispatchId;

    use super::{RemoteCheckoutPath, WorkspaceKey};

    #[test]
    fn derives_dispatch_artifact_locations() {
        let checkout_path = RemoteCheckoutPath::for_workspace(
            "~/workspace",
            &WorkspaceKey::new("project-a").unwrap(),
        );
        let dispatch_id = DispatchId::new("dispatch-123").unwrap();

        assert_eq!(checkout_path, "~/workspace/project-a/project-a");
        assert_eq!(
            checkout_path.task_worktree(&dispatch_id),
            "~/workspace/project-a/worktrees/dispatch-123"
        );
        assert_eq!(
            checkout_path.review_worktree(&dispatch_id),
            "~/workspace/project-a/review-worktrees/dispatch-123"
        );
        assert_eq!(
            checkout_path.task_run_directory(&dispatch_id),
            "~/workspace/project-a/dispatches/dispatch-123"
        );
        assert_eq!(
            checkout_path.review_run_directory(&dispatch_id),
            "~/workspace/project-a/review-runs/dispatch-123"
        );
    }
}
