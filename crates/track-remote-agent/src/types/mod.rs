//! Type definitions for the remote-agent crate.

mod dispatch;
mod events;
mod github;

pub(crate) use dispatch::{
    ClaudeStructuredOutputEnvelope, RemoteArtifactCleanupCounts, RemoteArtifactCleanupReport,
    RemoteDispatchSnapshot, RemoteProjectRegistryEntry, RemoteProjectRegistryFile,
    RemoteTaskCleanupMode, RemoteWorkspaceResetReport,
};
pub use events::{RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation};
pub(crate) use github::{
    GithubPullRequestApiResponse, GithubPullRequestMetadata, GithubPullRequestReference,
    GithubPullRequestReviewState, GithubReviewApiResponse, GithubSubmittedReview,
};
