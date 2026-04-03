//! Remote actions define the semantic operations that `track-remote-agent`
//! can perform on the remote machine over SSH.
//!
//! Each action owns the script contract and output interpretation for one
//! logical remote capability, so service code can depend on named operations
//! instead of mixing domain intent with raw shell execution details.

mod checkout;
mod dispatch;
mod files;
mod github;
mod maintenance;
mod registry;

pub(crate) use checkout::{
    CreateReviewWorktreeAction, CreateWorktreeAction, EnsureCheckoutAction,
    EnsureFollowUpWorktreeAction,
};
pub(crate) use dispatch::{
    CancelRemoteDispatchAction, LaunchRemoteDispatchAction, ReadDispatchSnapshotsAction,
};
pub(crate) use files::{ReadRemoteFileAction, UploadRemoteFileAction};
pub(crate) use github::{
    FetchGithubLoginAction, FetchPullRequestMetadataAction, FetchPullRequestReviewStateAction,
    PostPullRequestCommentAction,
};
pub(crate) use maintenance::{
    CleanupOrphanedRemoteArtifactsAction, CleanupReviewArtifactsAction,
    CleanupReviewWorkspaceCachesAction, CleanupTaskArtifactsAction, ResetWorkspaceAction,
};
pub(crate) use registry::{LoadRemoteRegistryAction, WriteRemoteRegistryAction};
