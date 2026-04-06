//! Types that describe noteworthy outcomes from remote follow-up reconciliation.

use track_types::time_utils::format_iso_8601_millis;
use track_types::types::TaskDispatchRecord;
use track_types::urls::Url;

use crate::types::GithubPullRequestReviewState;

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
    pub pull_request_url: Option<Url>,
    pub reviewer: String,
    pub pr_is_open: Option<bool>,
    pub pr_head_oid: Option<String>,
    pub latest_review_state: Option<String>,
    pub latest_review_submitted_at: Option<String>,
}

impl RemoteReviewFollowUpEvent {
    pub(crate) fn new(
        outcome: &str,
        detail: impl Into<String>,
        dispatch_record: &TaskDispatchRecord,
        reviewer: &str,
        pull_request_state: Option<&GithubPullRequestReviewState>,
    ) -> Self {
        let latest_review_state = pull_request_state
            .and_then(|state| state.latest_eligible_review.as_ref())
            .map(|review| review.state.clone());
        let latest_review_submitted_at = pull_request_state
            .and_then(|state| state.latest_eligible_review.as_ref())
            .map(|review| format_iso_8601_millis(review.submitted_at));

        Self {
            outcome: outcome.to_owned(),
            detail: detail.into(),
            task_id: dispatch_record.task_id.to_string(),
            dispatch_id: dispatch_record.dispatch_id.to_string(),
            dispatch_status: dispatch_record.status.as_str().to_owned(),
            remote_host: dispatch_record.remote_host.clone(),
            branch_name: dispatch_record
                .branch_name
                .clone()
                .map(|branch_name| branch_name.into_inner()),
            pull_request_url: dispatch_record.pull_request_url.clone(),
            reviewer: reviewer.to_owned(),
            pr_is_open: pull_request_state.map(|state| state.is_open),
            pr_head_oid: pull_request_state.map(|state| state.head_oid.clone()),
            latest_review_state,
            latest_review_submitted_at,
        }
    }
}
