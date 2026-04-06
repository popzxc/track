use crate::errors::{ErrorCode, TrackError};

mod dispatch_branch;
mod dispatch_run_directory;
mod dispatch_worktree_path;
mod remote_checkout_path;
mod workspace_key;

pub use dispatch_branch::DispatchBranch;
pub use dispatch_run_directory::DispatchRunDirectory;
pub use dispatch_worktree_path::DispatchWorktreePath;
pub use remote_checkout_path::RemoteCheckoutPath;
pub use workspace_key::WorkspaceKey;

pub(super) const TASK_BRANCH_PREFIX: &str = "track/";
pub(super) const REVIEW_BRANCH_PREFIX: &str = "track-review/";
pub(super) const TASK_WORKTREE_DIRECTORY_NAME: &str = "worktrees";
pub(super) const REVIEW_WORKTREE_DIRECTORY_NAME: &str = "review-worktrees";
pub(super) const TASK_RUN_DIRECTORY_NAME: &str = "dispatches";
pub(super) const REVIEW_RUN_DIRECTORY_NAME: &str = "review-runs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchLayoutKind {
    Task,
    Review,
}

impl DispatchLayoutKind {
    fn run_directory_name(self) -> &'static str {
        match self {
            Self::Task => TASK_RUN_DIRECTORY_NAME,
            Self::Review => REVIEW_RUN_DIRECTORY_NAME,
        }
    }
}

macro_rules! impl_string_value {
    ($name:ident) => {
        impl $name {
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
    };
}

pub(super) use impl_string_value;

fn invalid_remote_layout(field_name: &str, detail: &str) -> TrackError {
    TrackError::new(
        ErrorCode::InvalidPathComponent,
        format!("{field_name} {detail}"),
    )
}
