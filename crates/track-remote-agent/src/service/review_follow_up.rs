use std::sync::Arc;

use track_dal::database::DatabaseContext;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::task_repository::FileTaskRepository;
use track_types::errors::TrackError;
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{Status, TaskDispatchRecord};

use crate::prompts::RemoteDispatchPrompt;
use crate::types::{RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation};
use crate::utils::{build_review_follow_up_notification_comment, contextualize_track_error};
use crate::RemoteWorkspace;

use super::dispatch::RemoteDispatchService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReviewNotificationDecision {
    SkipMissingHeadOid,
    SkipAlreadyRecorded,
    Post,
}

fn review_notification_decision(
    dispatch_record: &TaskDispatchRecord,
    head_oid: &str,
    review_user: &str,
) -> ReviewNotificationDecision {
    let trimmed_head_oid = head_oid.trim();
    if trimmed_head_oid.is_empty() {
        return ReviewNotificationDecision::SkipMissingHeadOid;
    }

    let already_recorded_for_head = dispatch_record.review_request_head_oid.as_deref()
        == Some(trimmed_head_oid)
        && dispatch_record.review_request_user.as_deref() == Some(review_user);
    if already_recorded_for_head {
        ReviewNotificationDecision::SkipAlreadyRecorded
    } else {
        ReviewNotificationDecision::Post
    }
}

pub struct ReviewFollowUpService<'a> {
    database: &'a DatabaseContext,
    workspace: Arc<RemoteWorkspace>,
}

impl<'a> ReviewFollowUpService<'a> {
    pub(crate) fn new(database: &'a DatabaseContext, workspace: Arc<RemoteWorkspace>) -> Self {
        Self {
            database,
            workspace,
        }
    }

    fn dispatch_repository(&self) -> DispatchRepository<'a> {
        self.database.dispatch_repository()
    }

    fn task_repository(&self) -> FileTaskRepository<'a> {
        self.database.task_repository()
    }

    fn dispatch(&self) -> RemoteDispatchService<'a> {
        RemoteDispatchService {
            database: self.database,
            workspace: Arc::clone(&self.workspace),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn reconcile_review_follow_up(
        &self,
    ) -> Result<RemoteReviewFollowUpReconciliation, TrackError> {
        let remote_agent = self.workspace.remote_agent();
        let Some(review_follow_up) = remote_agent.review_follow_up.clone() else {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        };

        let task_ids = self.dispatch_repository().task_ids_with_history().await?;
        if task_ids.is_empty() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let dispatch_service = self.dispatch();
        let latest_dispatches = dispatch_service
            .latest_dispatches_for_tasks(&task_ids)
            .await?;
        let tasks_by_id = self
            .task_repository()
            .tasks_by_ids(
                &latest_dispatches
                    .iter()
                    .map(|dispatch_record| dispatch_record.task_id.clone())
                    .collect::<Vec<_>>(),
            )
            .await?
            .into_iter()
            .map(|task| (task.id.clone(), task))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut reconciliation = RemoteReviewFollowUpReconciliation::default();

        for dispatch_record in latest_dispatches {
            let Some(pull_request_url) = dispatch_record.pull_request_url.as_ref() else {
                continue;
            };

            let Some(task) = tasks_by_id.get(&dispatch_record.task_id) else {
                continue;
            };
            if task.status != Status::Open {
                continue;
            }

            let pull_request_state = self
                .workspace
                .projects()
                .fetch_pull_request_review_state(pull_request_url, &review_follow_up.main_user)
                .await
                .map_err(|error| {
                    contextualize_track_error(
                        error,
                        format!(
                            "Review follow-up could not inspect task {} PR {} for reviewer @{}",
                            dispatch_record.task_id, pull_request_url, review_follow_up.main_user
                        ),
                    )
                });
            let pull_request_state = match pull_request_state {
                Ok(pull_request_state) => pull_request_state,
                Err(error) => {
                    reconciliation.failures += 1;
                    tracing::warn!(
                        task_id = %dispatch_record.task_id,
                        dispatch_id = %dispatch_record.run.dispatch_id,
                        pull_request_url = %pull_request_url,
                        reviewer = %review_follow_up.main_user,
                        error = %error,
                        "Automatic PR follow-up inspection failed"
                    );
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "fetch_failed",
                        error.to_string(),
                        &dispatch_record,
                        &review_follow_up.main_user,
                        None,
                    ));
                    continue;
                }
            };

            reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                "task_evaluated",
                "Fetched PR review state for automatic follow-up reconciliation.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            if !pull_request_state.is_open {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "skip_closed_pr",
                    "Skipped automatic follow-up because the PR is not open anymore.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            if dispatch_record.run.status.is_active() {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "skip_active_dispatch",
                    "Skipped automatic follow-up because the latest dispatch is still active.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            if let Some(latest_review) = pull_request_state.latest_eligible_review.as_ref() {
                if latest_review.submitted_at > dispatch_record.run.created_at {
                    let follow_up_request = RemoteDispatchPrompt::build_review_follow_up_request(
                        pull_request_url,
                        &review_follow_up.main_user,
                        dispatch_record.run.created_at,
                    );
                    let queued_dispatch = dispatch_service
                        .queue_follow_up_dispatch(&dispatch_record.task_id, &follow_up_request)
                        .await?;
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "queue_follow_up",
                        format!(
                            "Queued a follow-up dispatch because reviewer @{} submitted {} at {} after dispatch {} started.",
                            review_follow_up.main_user,
                            latest_review.state,
                            format_iso_8601_millis(latest_review.submitted_at),
                            dispatch_record.run.dispatch_id,
                        ),
                        &queued_dispatch,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    tracing::info!(
                        task_id = %queued_dispatch.task_id,
                        dispatch_id = %queued_dispatch.run.dispatch_id,
                        reviewer = %review_follow_up.main_user,
                        pull_request_url = %pull_request_url,
                        "Queued automatic follow-up dispatch from PR review state"
                    );
                    self.mark_review_notification_for_head(
                        &queued_dispatch,
                        &pull_request_state.head_oid,
                        &review_follow_up.main_user,
                    )
                    .await?;
                    reconciliation.queued_dispatches.push(queued_dispatch);
                    continue;
                }
            }

            let head_oid = pull_request_state.head_oid.trim();
            match review_notification_decision(
                &dispatch_record,
                head_oid,
                &review_follow_up.main_user,
            ) {
                ReviewNotificationDecision::SkipMissingHeadOid => {
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "skip_missing_head_oid",
                        "Skipped PR reviewer notification because the PR head SHA is missing.",
                        &dispatch_record,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    continue;
                }
                ReviewNotificationDecision::SkipAlreadyRecorded => {
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "skip_notification_already_recorded",
                        "Skipped PR reviewer notification because this PR head already recorded the same reviewer.",
                        &dispatch_record,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    continue;
                }
                ReviewNotificationDecision::Post => {}
            }

            let notification_comment =
                build_review_follow_up_notification_comment(&review_follow_up.main_user, head_oid);
            let notify_reviewer_result = self
                .workspace
                .projects()
                .post_pull_request_comment(pull_request_url, &notification_comment)
                .await
                .map_err(|error| {
                    contextualize_track_error(
                        error,
                        format!(
                            "Review follow-up could not notify reviewer @{} for task {} PR {}",
                            review_follow_up.main_user, dispatch_record.task_id, pull_request_url
                        ),
                    )
                });
            if let Err(error) = notify_reviewer_result {
                reconciliation.failures += 1;
                tracing::warn!(
                    task_id = %dispatch_record.task_id,
                    dispatch_id = %dispatch_record.run.dispatch_id,
                    reviewer = %review_follow_up.main_user,
                    pull_request_url = %pull_request_url,
                    error = %error,
                    "Posting PR reviewer notification failed"
                );
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "notify_reviewer_failed",
                    error.to_string(),
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }
            self.mark_review_notification_for_head(
                &dispatch_record,
                head_oid,
                &review_follow_up.main_user,
            )
            .await?;
            reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                "notify_reviewer_posted",
                "Posted a PR comment mentioning the configured main GitHub user for the current PR head.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            reconciliation.review_notifications_updated += 1;
            tracing::info!(
                task_id = %dispatch_record.task_id,
                dispatch_id = %dispatch_record.run.dispatch_id,
                reviewer = %review_follow_up.main_user,
                pull_request_url = %pull_request_url,
                "Posted PR reviewer follow-up notification"
            );
        }

        tracing::info!(
            review_notifications_updated = reconciliation.review_notifications_updated,
            queued_dispatches = reconciliation.queued_dispatches.len(),
            failures = reconciliation.failures,
            evaluated_events = reconciliation.events.len(),
            "Completed review follow-up reconciliation"
        );

        Ok(reconciliation)
    }

    async fn mark_review_notification_for_head(
        &self,
        dispatch_record: &TaskDispatchRecord,
        head_oid: &str,
        review_user: &str,
    ) -> Result<(), TrackError> {
        let mut updated_record = dispatch_record.clone();

        // These persisted field names started out as "review request" markers.
        // We intentionally keep them for backward compatibility with existing
        // dispatch JSON while reusing them as a "notified this reviewer about
        // this PR head already" checkpoint.
        updated_record.review_request_head_oid = Some(head_oid.to_owned());
        updated_record.review_request_user = Some(review_user.to_owned());
        self.dispatch_repository()
            .save_dispatch(&updated_record)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{review_notification_decision, ReviewNotificationDecision};
    use track_types::ids::{DispatchId, ProjectId, TaskId};
    use track_types::time_utils::now_utc;
    use track_types::types::{
        DispatchStatus, RemoteAgentPreferredTool, RemoteRunState, TaskDispatchRecord,
    };

    fn sample_dispatch_record() -> TaskDispatchRecord {
        let dispatch_id = DispatchId::new("dispatch-1").expect("dispatch ids should parse");
        let timestamp = now_utc();
        TaskDispatchRecord {
            run: RemoteRunState {
                dispatch_id,
                preferred_tool: RemoteAgentPreferredTool::Codex,
                status: DispatchStatus::Succeeded,
                created_at: timestamp,
                updated_at: timestamp,
                finished_at: Some(timestamp),
                remote_host: "127.0.0.1".to_owned(),
                branch_name: None,
                worktree_path: None,
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: None,
            },
            task_id: TaskId::new("task-1").expect("task ids should parse"),
            project: ProjectId::new("project-a").expect("project ids should parse"),
            pull_request_url: None,
            review_request_head_oid: None,
            review_request_user: None,
        }
    }

    #[test]
    fn initial_dispatch_is_notified_when_no_tracking_checkpoint_exists() {
        let dispatch_record = sample_dispatch_record();

        let decision = review_notification_decision(&dispatch_record, "abc123", "octocat");

        assert_eq!(decision, ReviewNotificationDecision::Post);
    }

    #[test]
    fn duplicate_notification_is_skipped_for_same_head_and_user() {
        let mut dispatch_record = sample_dispatch_record();
        dispatch_record.review_request_head_oid = Some("abc123".to_owned());
        dispatch_record.review_request_user = Some("octocat".to_owned());

        let decision = review_notification_decision(&dispatch_record, "abc123", "octocat");

        assert_eq!(decision, ReviewNotificationDecision::SkipAlreadyRecorded);
    }

    #[test]
    fn notification_posts_again_when_head_or_user_changes() {
        let mut dispatch_record = sample_dispatch_record();
        dispatch_record.review_request_head_oid = Some("abc123".to_owned());
        dispatch_record.review_request_user = Some("octocat".to_owned());

        let decision_for_new_head =
            review_notification_decision(&dispatch_record, "def456", "octocat");
        let decision_for_new_user =
            review_notification_decision(&dispatch_record, "abc123", "hubot");

        assert_eq!(decision_for_new_head, ReviewNotificationDecision::Post);
        assert_eq!(decision_for_new_user, ReviewNotificationDecision::Post);
    }
}
