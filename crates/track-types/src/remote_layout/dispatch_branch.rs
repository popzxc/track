use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::TrackError;
use crate::ids::DispatchId;

use super::{impl_string_value, parse_dispatch_branch, REVIEW_BRANCH_PREFIX, TASK_BRANCH_PREFIX};

/// Fully qualified Git branch name reserved for one remote dispatch or review
/// run, for example `track/<dispatch-id>` or `track-review/<dispatch-id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DispatchBranch(String);

impl DispatchBranch {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
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

    pub fn dispatch_id(&self) -> DispatchId {
        let (_kind, dispatch_id) =
            parse_dispatch_branch(self.as_str()).expect("dispatch branches should stay valid");
        DispatchId::new(dispatch_id).expect("dispatch branch suffix should be a valid dispatch id")
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
        Self::new(&value).map_err(D::Error::custom)
    }
}

impl_string_value!(DispatchBranch);

#[cfg(test)]
mod tests {
    use crate::errors::ErrorCode;
    use crate::ids::DispatchId;

    use super::DispatchBranch;

    #[test]
    fn builders_enforce_task_and_review_prefixes() {
        let dispatch_id = DispatchId::new("dispatch-123").unwrap();

        assert_eq!(
            DispatchBranch::for_task(&dispatch_id).as_str(),
            "track/dispatch-123"
        );
        assert_eq!(
            DispatchBranch::for_review(&dispatch_id).as_str(),
            "track-review/dispatch-123"
        );
    }

    #[test]
    fn parser_rejects_non_track_namespaces() {
        let error = DispatchBranch::new("feature/dispatch-123").unwrap_err();

        assert_eq!(error.code, ErrorCode::InvalidPathComponent);
    }
}
