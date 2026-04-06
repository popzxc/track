use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::TrackError;
use crate::ids::{DispatchId, ProjectId};
use crate::remote_layout::{invalid_remote_layout, DispatchLayoutKind};

use super::{
    impl_string_value, DispatchRunDirectory, WorkspaceKey, REVIEW_WORKTREE_DIRECTORY_NAME,
    TASK_WORKTREE_DIRECTORY_NAME,
};

/// Absolute remote path to the dedicated Git worktree that one dispatch uses
/// while preparing or executing a task or review run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DispatchWorktreePath(String);

impl DispatchWorktreePath {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
        let trimmed = value.as_ref().trim();
        parse_dispatch_layout_path(trimmed, "Dispatch worktree path")?;

        Ok(Self(trimmed.to_owned()))
    }

    pub fn for_task(workspace_root: &str, project: &ProjectId, dispatch_id: &DispatchId) -> Self {
        Self(format!(
            "{}/{}/{}/{}",
            workspace_root.trim_end_matches('/'),
            project,
            TASK_WORKTREE_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn for_review(
        workspace_root: &str,
        workspace_key: &WorkspaceKey,
        dispatch_id: &DispatchId,
    ) -> Self {
        Self(format!(
            "{}/{}/{}/{}",
            workspace_root.trim_end_matches('/'),
            workspace_key,
            REVIEW_WORKTREE_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn dispatch_id(&self) -> DispatchId {
        let (_kind, _prefix, dispatch_id) =
            parse_dispatch_layout_path(self.as_str(), "Dispatch worktree path")
                .expect("dispatch worktree paths should stay valid");
        DispatchId::new(dispatch_id)
            .expect("dispatch worktree paths should end with a valid dispatch id")
    }

    pub fn run_directory(&self) -> DispatchRunDirectory {
        self.run_directory_for(&self.dispatch_id())
    }

    pub fn run_directory_for(&self, dispatch_id: &DispatchId) -> DispatchRunDirectory {
        let (kind, prefix, _) = parse_dispatch_layout_path(self.as_str(), "Dispatch worktree path")
            .expect("dispatch worktree paths should stay valid");

        DispatchRunDirectory::from_layout(format!(
            "{prefix}/{}/{dispatch_id}",
            kind.run_directory_name()
        ))
    }

    pub fn from_db_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(super) fn from_layout(value: String) -> Self {
        Self(value)
    }
}

impl<'de> Deserialize<'de> for DispatchWorktreePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(D::Error::custom)
    }
}

impl_string_value!(DispatchWorktreePath);

fn parse_dispatch_layout_path<'a>(
    value: &'a str,
    field_name: &str,
) -> Result<(DispatchLayoutKind, &'a str, &'a str), TrackError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_remote_layout(field_name, "must not be empty."));
    }

    if let Some((prefix, dispatch_id)) =
        trimmed.rsplit_once(&format!("/{TASK_WORKTREE_DIRECTORY_NAME}/"))
    {
        if prefix.is_empty() {
            return Err(invalid_remote_layout(
                field_name,
                "must include a workspace prefix before the dispatch directory.",
            ));
        }
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                field_name,
                "must end with a valid dispatch id under the task worktree directory.",
            )
        })?;
        return Ok((DispatchLayoutKind::Task, prefix, dispatch_id));
    }

    if let Some((prefix, dispatch_id)) =
        trimmed.rsplit_once(&format!("/{REVIEW_WORKTREE_DIRECTORY_NAME}/"))
    {
        if prefix.is_empty() {
            return Err(invalid_remote_layout(
                field_name,
                "must include a workspace prefix before the dispatch directory.",
            ));
        }
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                field_name,
                "must end with a valid dispatch id under the review worktree directory.",
            )
        })?;
        return Ok((DispatchLayoutKind::Review, prefix, dispatch_id));
    }

    Err(invalid_remote_layout(
        field_name,
        "must live under `worktrees/<dispatch-id>` or `review-worktrees/<dispatch-id>`.",
    ))
}

#[cfg(test)]
mod tests {
    use crate::ids::{DispatchId, ProjectId};

    use super::{DispatchWorktreePath, WorkspaceKey};

    #[test]
    fn builders_keep_task_and_review_layouts() {
        let dispatch_id = DispatchId::new("dispatch-123").unwrap();
        let project = ProjectId::new("project-a").unwrap();
        let workspace_key = WorkspaceKey::new("review-a").unwrap();

        assert_eq!(
            DispatchWorktreePath::for_task("~/workspace", &project, &dispatch_id).as_str(),
            "~/workspace/project-a/worktrees/dispatch-123"
        );
        assert_eq!(
            DispatchWorktreePath::for_review("~/workspace", &workspace_key, &dispatch_id).as_str(),
            "~/workspace/review-a/review-worktrees/dispatch-123"
        );
    }

    #[test]
    fn derive_run_directories() {
        let worktree_path =
            DispatchWorktreePath::new("~/workspace/project-a/review-worktrees/dispatch-123")
                .unwrap();

        assert_eq!(
            worktree_path.run_directory().as_str(),
            "~/workspace/project-a/review-runs/dispatch-123"
        );
    }

    #[test]
    fn follow_up_dispatches_keep_the_reused_worktree_but_get_a_fresh_run_directory() {
        let worktree_path =
            DispatchWorktreePath::new("~/workspace/project-a/worktrees/dispatch-1").unwrap();
        let follow_up_dispatch_id = DispatchId::new("dispatch-2").unwrap();

        assert_eq!(
            worktree_path
                .run_directory_for(&follow_up_dispatch_id)
                .as_str(),
            "~/workspace/project-a/dispatches/dispatch-2"
        );
    }
}
