use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;

// =============================================================================
// Strongly Typed Storage Identifiers
// =============================================================================
//
// Task, review, dispatch, and project identifiers all share the same safety
// contract: they eventually become path components or stable lookup keys, so
// once one has been accepted we should not keep re-validating the same string
// at every repository call. These wrappers move that invariant into the type
// system and keep validation at the edges instead of scattered through the app.
macro_rules! define_path_id {
    ($name:ident, $field_name:literal, $db_expectation:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Result<Self, TrackError> {
                let value = validate_single_normal_path_component(
                    value.as_ref(),
                    $field_name,
                    ErrorCode::InvalidPathComponent,
                )?;

                Ok(Self(value))
            }

            pub fn from_db(value: impl Into<String>) -> Self {
                let value = value.into();
                Self::new(&value).expect($db_expectation)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str() == other
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.into_inner()
            }
        }

        impl TryFrom<String> for $name {
            type Error = TrackError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(&value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = TrackError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(&value).map_err(D::Error::custom)
            }
        }
    };
}

define_path_id!(
    TaskId,
    "Task id",
    "database task ids should be valid path components"
);
define_path_id!(
    ProjectId,
    "Project name",
    "database project names should be valid path components"
);
define_path_id!(
    ReviewId,
    "Review id",
    "database review ids should be valid path components"
);
define_path_id!(
    DispatchId,
    "Dispatch id",
    "database dispatch ids should be valid path components"
);

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DispatchId, ProjectId};

    #[test]
    fn trims_and_validates_path_ids() {
        let project_id = ProjectId::new(" project-a ").expect("path ids should validate");

        assert_eq!(project_id.as_str(), "project-a");
    }

    #[test]
    fn rejects_invalid_path_id_shapes() {
        let error = DispatchId::new("../escape").expect_err("invalid path ids should fail");

        assert_eq!(error.code, crate::errors::ErrorCode::InvalidPathComponent);
    }

    #[test]
    fn serde_rejects_invalid_identifier_values() {
        let error = serde_json::from_value::<ProjectId>(json!("a/b"))
            .expect_err("serde should reject invalid ids");

        assert!(error
            .to_string()
            .contains("must be one non-empty path component"));
    }
}
