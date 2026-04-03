//! Types that describe noteworthy outcomes from remote follow-up reconciliation.

use track_types::types::TaskDispatchRecord;

/// Summarizes one reconciliation pass over saved review follow-up state.
///
/// The reconciliation step may decide to queue new dispatches, refresh local
/// notifications, or report failures. This struct captures those logical
/// outcomes without exposing the internal control flow that produced them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteReviewFollowUpReconciliation {
    pub queued_dispatches: Vec<TaskDispatchRecord>,
    pub review_notifications_updated: usize,
    pub failures: usize,
    pub events: Vec<RemoteReviewFollowUpEvent>,
}

/// Describes one meaningful outcome observed while reconciling a remote review
/// follow-up.
///
/// These events are the explanation layer for higher-level callers: they record
/// what happened remotely or on GitHub, and why the system chose to queue,
/// update, skip, or fail follow-up work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteReviewFollowUpEvent {
    pub outcome: String,
    pub detail: String,
    pub task_id: String,
    pub dispatch_id: String,
    pub dispatch_status: String,
    pub remote_host: String,
    pub branch_name: Option<String>,
    pub pull_request_url: Option<String>,
    pub reviewer: String,
    pub pr_is_open: Option<bool>,
    pub pr_head_oid: Option<String>,
    pub latest_review_state: Option<String>,
    pub latest_review_submitted_at: Option<String>,
}
