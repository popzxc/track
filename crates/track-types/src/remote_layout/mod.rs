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

const TASK_BRANCH_PREFIX: &str = "track/";
const REVIEW_BRANCH_PREFIX: &str = "track-review/";
const TASK_WORKTREE_DIRECTORY_NAME: &str = "worktrees";
const REVIEW_WORKTREE_DIRECTORY_NAME: &str = "review-worktrees";
const TASK_RUN_DIRECTORY_NAME: &str = "dispatches";
const REVIEW_RUN_DIRECTORY_NAME: &str = "review-runs";

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
            /// Consumes this strong value at an application boundary and
            /// returns the underlying string representation.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.0 == *other
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
