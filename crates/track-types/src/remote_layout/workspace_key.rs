use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::{ErrorCode, TrackError};
use crate::ids::ProjectId;
use crate::path_component::validate_single_normal_path_component;

use super::impl_string_value;

/// Stable remote workspace slug used to group one repository's checkout and
/// derived review/task artifacts under the remote workspace root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkspaceKey(String);

impl WorkspaceKey {
    pub fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
        let value = validate_single_normal_path_component(
            value.as_ref(),
            "Workspace key",
            ErrorCode::InvalidPathComponent,
        )?;

        Ok(Self(value))
    }

    pub fn from_repository_full_name(repository_full_name: &str) -> Self {
        let slug = slug::slugify(repository_full_name.replace('/', "-").trim());
        let fallback = if slug.is_empty() { "review-repo" } else { &slug };

        Self::new(fallback).expect("generated workspace keys should be valid path components")
    }

    pub fn from_db_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl From<ProjectId> for WorkspaceKey {
    fn from(value: ProjectId) -> Self {
        Self(value.into_inner())
    }
}

impl From<&ProjectId> for WorkspaceKey {
    fn from(value: &ProjectId) -> Self {
        Self(value.as_str().to_owned())
    }
}

impl<'de> Deserialize<'de> for WorkspaceKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(&value).map_err(D::Error::custom)
    }
}

impl_string_value!(WorkspaceKey);

#[cfg(test)]
mod tests {
    use super::WorkspaceKey;

    #[test]
    fn validates_single_normal_components() {
        let workspace_key = WorkspaceKey::new(" project-a ").unwrap();

        assert_eq!(workspace_key.as_str(), "project-a");
    }

    #[test]
    fn can_be_derived_from_repository_names() {
        let workspace_key = WorkspaceKey::from_repository_full_name("acme/project-x");

        assert_eq!(workspace_key.as_str(), "acme-project-x");
    }
}
