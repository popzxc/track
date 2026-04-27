use time::{Duration, OffsetDateTime};
use track_types::errors::TrackError;
use track_types::time_utils::now_utc;
use track_types::types::{DispatchStatus, RemoteRunState};

use crate::RemoteRunSnapshotView;

// =============================================================================
// Remote Run Snapshot Policy
// =============================================================================
//
// Task dispatches and PR review runs have different domain payloads, but their
// remote sidecar lifecycle is the same: missing, running, canceled, completed,
// or failed snapshots should lead to the same local state transition shape.
// This module keeps that policy pure so task/review services can stay readable
// and continue to own persistence, logging, and domain-specific outcome fields.
pub(in crate::service) enum RemoteRunSnapshotAction<Outcome> {
    Unchanged,
    MarkRunning {
        refreshed_at: OffsetDateTime,
    },
    MarkCanceled {
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
    },
    MarkFailed {
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
        error_message: String,
    },
    ApplyCompletedOutcome {
        refreshed_at: OffsetDateTime,
        finished_at: OffsetDateTime,
        outcome: Outcome,
    },
}

pub(in crate::service) struct RemoteRunSnapshotPolicy {
    stale_after: Duration,
    preparing_stale_error_message: &'static str,
    completed_missing_message: &'static str,
    failed_fallback_message: &'static str,
}

impl RemoteRunSnapshotPolicy {
    pub(in crate::service) fn new(
        stale_after: Duration,
        preparing_stale_error_message: &'static str,
        completed_missing_message: &'static str,
        failed_fallback_message: &'static str,
    ) -> Self {
        Self {
            stale_after,
            preparing_stale_error_message,
            completed_missing_message,
            failed_fallback_message,
        }
    }

    pub(in crate::service) fn reconcile<Outcome>(
        &self,
        run: &RemoteRunState,
        snapshot: &RemoteRunSnapshotView,
        parse_completed: impl FnOnce(&str) -> Result<Outcome, TrackError>,
    ) -> Result<RemoteRunSnapshotAction<Outcome>, TrackError> {
        let refreshed_at = now_utc();

        if snapshot.is_missing() {
            let preparing_stale = run.status == DispatchStatus::Preparing
                && refreshed_at - run.updated_at >= self.stale_after;
            if preparing_stale {
                return Ok(RemoteRunSnapshotAction::MarkFailed {
                    refreshed_at,
                    finished_at: refreshed_at,
                    error_message: self.preparing_stale_error_message.to_owned(),
                });
            }

            return Ok(RemoteRunSnapshotAction::Unchanged);
        }

        if snapshot.is_running() {
            return Ok(RemoteRunSnapshotAction::MarkRunning { refreshed_at });
        }

        if snapshot.is_canceled() {
            return Ok(RemoteRunSnapshotAction::MarkCanceled {
                refreshed_at,
                finished_at: snapshot.finished_at_or(refreshed_at),
            });
        }

        let finished_at = snapshot.finished_at_or(refreshed_at);
        if snapshot.is_completed() {
            let remote_result = snapshot.required_result(self.completed_missing_message)?;
            let outcome = parse_completed(remote_result)?;
            return Ok(RemoteRunSnapshotAction::ApplyCompletedOutcome {
                refreshed_at,
                finished_at,
                outcome,
            });
        }

        Ok(RemoteRunSnapshotAction::MarkFailed {
            refreshed_at,
            finished_at,
            error_message: snapshot.failure_message(self.failed_fallback_message),
        })
    }
}

#[cfg(test)]
mod tests {
    use track_types::ids::{DispatchId, ProjectId};
    use track_types::remote_layout::DispatchRunDirectory;
    use track_types::types::RemoteAgentPreferredTool;

    use crate::RemoteRunObservedStatus;

    use super::*;

    fn test_policy() -> RemoteRunSnapshotPolicy {
        RemoteRunSnapshotPolicy::new(
            Duration::minutes(5),
            "Preparing got stale.",
            "Completed without result.",
            "Run failed.",
        )
    }

    fn test_run(status: DispatchStatus, updated_at: OffsetDateTime) -> RemoteRunState {
        RemoteRunState {
            dispatch_id: DispatchId::new("dispatch-1").expect("dispatch id should parse"),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            status,
            created_at: updated_at,
            updated_at,
            finished_at: None,
            remote_host: "198.51.100.10".to_owned(),
            branch_name: None,
            worktree_path: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
        }
    }

    #[test]
    fn stale_missing_preparing_snapshot_becomes_failed_action() {
        let refreshed = now_utc();
        let updated_at = refreshed - Duration::minutes(10);
        let snapshot = RemoteRunSnapshotView::missing(DispatchRunDirectory::for_task(
            "~/workspace",
            &ProjectId::new("project-a").expect("project id should parse"),
            &DispatchId::new("dispatch-1").expect("dispatch id should parse"),
        ));
        let run = test_run(DispatchStatus::Preparing, updated_at);
        let action = test_policy()
            .reconcile(&run, &snapshot, |_result| Ok(()))
            .expect("snapshot should reconcile");

        match action {
            RemoteRunSnapshotAction::MarkFailed { error_message, .. } => {
                assert_eq!(error_message, "Preparing got stale.");
            }
            _ => panic!("stale preparing snapshot should be marked failed"),
        }
    }

    #[test]
    fn completed_snapshot_parses_domain_outcome() {
        let snapshot = RemoteRunSnapshotView {
            run_directory: DispatchRunDirectory::for_task(
                "~/workspace",
                &ProjectId::new("project-a").expect("project id should parse"),
                &DispatchId::new("dispatch-1").expect("dispatch id should parse"),
            ),
            status: RemoteRunObservedStatus::Completed,
            result: Some("done".to_owned()),
            stderr: None,
            finished_at: None,
        };
        let run = test_run(DispatchStatus::Running, now_utc());
        let action = test_policy()
            .reconcile(&run, &snapshot, |result| Ok(result.to_owned()))
            .expect("snapshot should reconcile");

        match action {
            RemoteRunSnapshotAction::ApplyCompletedOutcome { outcome, .. } => {
                assert_eq!(outcome, "done");
            }
            _ => panic!("completed snapshot should apply an outcome"),
        }
    }
}
