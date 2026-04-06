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
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Result<Self, crate::errors::TrackError> {
                let value = crate::path_component::validate_single_normal_path_component(
                    value.as_ref(),
                    $field_name,
                    crate::errors::ErrorCode::InvalidPathComponent,
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

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl std::borrow::Borrow<str> for $name {
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
            type Error = crate::errors::TrackError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(&value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = crate::errors::TrackError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

mod dispatch_id;
mod project_id;
mod review_id;
mod task_id;

pub use dispatch_id::DispatchId;
pub use project_id::ProjectId;
pub use review_id::ReviewId;
pub use task_id::TaskId;
