use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::TrackError;
use crate::ids::DispatchId;
use crate::remote_layout::{invalid_remote_layout, DispatchLayoutKind};

use super::{impl_string_value, REVIEW_BRANCH_PREFIX, TASK_BRANCH_PREFIX};

/// Fully qualified Git branch name reserved for one remote dispatch or review
/// run, for example `track/<dispatch-id>` or `track-review/<dispatch-id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DispatchBranch(String);

impl DispatchBranch {
    fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
        let trimmed = value.as_ref().trim();
        parse_dispatch_branch(trimmed)?;

        Ok(Self(trimmed.to_owned()))
    }

    pub fn for_task(dispatch_id: &DispatchId) -> Self {
        Self(format!("{TASK_BRANCH_PREFIX}{dispatch_id}"))
    }

    pub fn for_review(dispatch_id: &DispatchId) -> Self {
        Self(format!("{REVIEW_BRANCH_PREFIX}{dispatch_id}"))
    }

    pub fn from_db_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl<'de> Deserialize<'de> for DispatchBranch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        // TODO: Do we need this kind of validation here? Probably yes, need to validate.
        Self::new(&value).map_err(D::Error::custom)
    }
}

impl_string_value!(DispatchBranch);

fn parse_dispatch_branch(value: &str) -> Result<(DispatchLayoutKind, &str), TrackError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_remote_layout(
            "Dispatch branch",
            "must not be empty.",
        ));
    }

    if let Some(dispatch_id) = trimmed.strip_prefix(TASK_BRANCH_PREFIX) {
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                "Dispatch branch",
                "must match `track/<dispatch-id>` or `track-review/<dispatch-id>`.",
            )
        })?;
        return Ok((DispatchLayoutKind::Task, dispatch_id));
    }

    if let Some(dispatch_id) = trimmed.strip_prefix(REVIEW_BRANCH_PREFIX) {
        DispatchId::new(dispatch_id).map_err(|_| {
            invalid_remote_layout(
                "Dispatch branch",
                "must match `track/<dispatch-id>` or `track-review/<dispatch-id>`.",
            )
        })?;
        return Ok((DispatchLayoutKind::Review, dispatch_id));
    }

    Err(invalid_remote_layout(
        "Dispatch branch",
        "must match `track/<dispatch-id>` or `track-review/<dispatch-id>`.",
    ))
}

#[cfg(test)]
mod tests {
    use crate::errors::ErrorCode;
    use crate::ids::DispatchId;

    use super::DispatchBranch;

    #[test]
    fn builders_enforce_task_and_review_prefixes() {
        let dispatch_id = DispatchId::new("dispatch-123").unwrap();

        assert_eq!(DispatchBranch::for_task(&dispatch_id), "track/dispatch-123");
        assert_eq!(
            DispatchBranch::for_review(&dispatch_id),
            "track-review/dispatch-123"
        );
    }

    #[test]
    fn parser_rejects_non_track_namespaces() {
        let error = DispatchBranch::new("feature/dispatch-123").unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidPathComponent);
    }
}
