use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::TrackError;
use crate::ids::{DispatchId, ProjectId};
use crate::remote_layout::{invalid_remote_layout, DispatchLayoutKind};

use super::{impl_string_value, WorkspaceKey, REVIEW_RUN_DIRECTORY_NAME, TASK_RUN_DIRECTORY_NAME};

/// Absolute remote path to the sidecar directory that stores prompt, schema,
/// status, and result files for one dispatch attempt.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DispatchRunDirectory(String);

impl DispatchRunDirectory {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
        let trimmed = value.as_ref().trim();
        parse_dispatch_run_directory(trimmed)?;

        Ok(Self(trimmed.to_owned()))
    }

    pub fn for_task(workspace_root: &str, project: &ProjectId, dispatch_id: &DispatchId) -> Self {
        Self(format!(
            "{}/{}/{}/{}",
            workspace_root.trim_end_matches('/'),
            project,
            TASK_RUN_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn for_review(
        workspace_root: &str,
        workspace_key: &WorkspaceKey,
        dispatch_id: &DispatchId,
    ) -> Self {
        let workspace_key = workspace_key.clone().into_inner();
        Self(format!(
            "{}/{}/{}/{}",
            workspace_root.trim_end_matches('/'),
            workspace_key,
            REVIEW_RUN_DIRECTORY_NAME,
            dispatch_id
        ))
    }

    pub fn dispatch_id(&self) -> DispatchId {
        let (_kind, _prefix, dispatch_id) = parse_dispatch_run_directory(&self.0)
            .expect("dispatch run directories should stay valid");
        DispatchId::new(dispatch_id)
            .expect("dispatch run directories should end with a valid dispatch id")
    }

    pub fn join(&self, file_name: &str) -> String {
        format!("{}/{}", self.0.trim_end_matches('/'), file_name)
    }

    pub fn from_db_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(super) fn from_layout(value: String) -> Self {
        Self(value)
    }
}

impl<'de> Deserialize<'de> for DispatchRunDirectory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(D::Error::custom)
    }
}

impl_string_value!(DispatchRunDirectory);

fn parse_dispatch_run_directory(
    value: &str,
) -> Result<(DispatchLayoutKind, &str, &str), TrackError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_remote_layout(
            "Dispatch run directory",
            "must not be empty.",
        ));
    }

    if let Some((prefix, dispatch_id)) =
        trimmed.rsplit_once(&format!("/{TASK_RUN_DIRECTORY_NAME}/"))
    {
        if prefix.is_empty() {
            return Err(invalid_remote_layout(
                "Dispatch run directory",
                "must include a workspace prefix before the dispatch directory.",
            ));
        }
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                "Dispatch run directory",
                "must end with a valid dispatch id under the task run directory.",
            )
        })?;
        return Ok((DispatchLayoutKind::Task, prefix, dispatch_id));
    }

    if let Some((prefix, dispatch_id)) =
        trimmed.rsplit_once(&format!("/{REVIEW_RUN_DIRECTORY_NAME}/"))
    {
        if prefix.is_empty() {
            return Err(invalid_remote_layout(
                "Dispatch run directory",
                "must include a workspace prefix before the dispatch directory.",
            ));
        }
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                "Dispatch run directory",
                "must end with a valid dispatch id under the review run directory.",
            )
        })?;
        return Ok((DispatchLayoutKind::Review, prefix, dispatch_id));
    }

    Err(invalid_remote_layout(
        "Dispatch run directory",
        "must live under `dispatches/<dispatch-id>` or `review-runs/<dispatch-id>`.",
    ))
}

#[cfg(test)]
mod tests {
    use crate::ids::{DispatchId, ProjectId};

    use super::{DispatchRunDirectory, WorkspaceKey};

    #[test]
    fn builders_keep_task_and_review_layouts() {
        let dispatch_id = DispatchId::new("dispatch-123").unwrap();
        let project = ProjectId::new("project-a").unwrap();
        let workspace_key = WorkspaceKey::new("review-a").unwrap();

        assert_eq!(
            DispatchRunDirectory::for_task("~/workspace", &project, &dispatch_id),
            "~/workspace/project-a/dispatches/dispatch-123"
        );
        assert_eq!(
            DispatchRunDirectory::for_review("~/workspace", &workspace_key, &dispatch_id),
            "~/workspace/review-a/review-runs/dispatch-123"
        );
    }

    #[test]
    fn joins_sidecar_files() {
        let run_directory =
            DispatchRunDirectory::new("~/workspace/project-a/dispatches/dispatch-123").unwrap();

        assert_eq!(
            run_directory.join("prompt.md"),
            "~/workspace/project-a/dispatches/dispatch-123/prompt.md"
        );
    }
}
