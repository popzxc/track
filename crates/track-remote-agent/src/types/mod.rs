//! Type definitions for the remote-agent crate.

mod dispatch;
mod events;
mod github;

pub(crate) use dispatch::{
    ClaudeStructuredOutputEnvelope, OpencodeStructuredOutput, RemoteArtifactCleanupCounts,
    RemoteArtifactCleanupReport, RemoteTaskCleanupMode, RemoteWorkspaceResetReport,
};
pub use events::{RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation};
pub(crate) use github::{
    GithubPullRequestApiResponse, GithubPullRequestMetadata, GithubPullRequestReference,
    GithubPullRequestReviewState, GithubReviewApiResponse, GithubSubmittedReview,
};
