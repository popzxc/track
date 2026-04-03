use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::format_iso_8601_millis;
use track_types::types::{ReviewRunRecord, Status, TaskDispatchRecord};

use crate::prompts::RemoteDispatchPrompt;
use crate::remote_actions::{FetchPullRequestReviewStateAction, PostPullRequestCommentAction};
use crate::ssh::SshClient;
use crate::types::{RemoteReviewFollowUpEvent, RemoteReviewFollowUpReconciliation};
use crate::utils::{build_review_follow_up_notification_comment, contextualize_track_error};

use super::RemoteDispatchService;

impl<'a> RemoteDispatchService<'a> {
    pub fn reconcile_review_follow_up(
        &self,
    ) -> Result<RemoteReviewFollowUpReconciliation, TrackError> {
        let remote_agent = match self.config_service.load_remote_agent_runtime_config() {
            Ok(config) => config,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::ConfigNotFound
                        | ErrorCode::InvalidConfig
                        | ErrorCode::InvalidRemoteAgentConfig
                ) =>
            {
                return Ok(RemoteReviewFollowUpReconciliation::default());
            }
            Err(error) => return Err(error),
        };
        let Some(remote_agent) = remote_agent else {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        };
        let Some(review_follow_up) = remote_agent.review_follow_up.clone() else {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        };
        if !remote_agent.managed_key_path.exists() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let task_ids = self.dispatch_repository.task_ids_with_history()?;
        if task_ids.is_empty() {
            return Ok(RemoteReviewFollowUpReconciliation::default());
        }

        let latest_dispatches = self.latest_dispatches_for_tasks(&task_ids)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let mut reconciliation = RemoteReviewFollowUpReconciliation::default();

        for dispatch_record in latest_dispatches {
            let Some(pull_request_url) = dispatch_record
                .pull_request_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };

            match self.task_repository.get_task(&dispatch_record.task_id) {
                Ok(task) if task.status == Status::Open => task,
                Ok(_) => continue,
                Err(error) if error.code == ErrorCode::TaskNotFound => continue,
                Err(error) => return Err(error),
            };

            let pull_request_state = FetchPullRequestReviewStateAction::new(
                &ssh_client,
                pull_request_url,
                &review_follow_up.main_user,
            )
            .execute()
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

            if dispatch_record.status.is_active() {
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
                if latest_review.submitted_at > dispatch_record.created_at {
                    let follow_up_request = RemoteDispatchPrompt::build_review_follow_up_request(
                        pull_request_url,
                        &review_follow_up.main_user,
                        dispatch_record.created_at,
                    );
                    let queued_dispatch = self
                        .queue_follow_up_dispatch(&dispatch_record.task_id, &follow_up_request)?;
                    reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                        "queue_follow_up",
                        format!(
                            "Queued a follow-up dispatch because reviewer @{} submitted {} at {} after dispatch {} started.",
                            review_follow_up.main_user,
                            latest_review.state,
                            format_iso_8601_millis(latest_review.submitted_at),
                            dispatch_record.dispatch_id,
                        ),
                        &queued_dispatch,
                        &review_follow_up.main_user,
                        Some(&pull_request_state),
                    ));
                    reconciliation.queued_dispatches.push(queued_dispatch);
                    continue;
                }
            }

            if pull_request_state.head_oid.is_empty() {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "skip_missing_head_oid",
                    "Skipped PR reviewer notification because the PR head SHA is missing.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            let already_recorded_for_head = dispatch_record.review_request_head_oid.as_deref()
                == Some(pull_request_state.head_oid.as_str())
                && dispatch_record.review_request_user.as_deref()
                    == Some(review_follow_up.main_user.as_str());
            if already_recorded_for_head {
                reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                    "skip_notification_already_recorded",
                    "Skipped PR reviewer notification because this PR head already recorded the same reviewer.",
                    &dispatch_record,
                    &review_follow_up.main_user,
                    Some(&pull_request_state),
                ));
                continue;
            }

            let notification_comment = build_review_follow_up_notification_comment(
                &review_follow_up.main_user,
                &pull_request_state.head_oid,
            );
            let notify_reviewer_result = PostPullRequestCommentAction::new(
                &ssh_client,
                pull_request_url,
                &notification_comment,
            )
            .execute()
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
                &pull_request_state.head_oid,
                &review_follow_up.main_user,
            )?;
            reconciliation.events.push(RemoteReviewFollowUpEvent::new(
                "notify_reviewer_posted",
                "Posted a PR comment mentioning the configured main GitHub user for the current PR head.",
                &dispatch_record,
                &review_follow_up.main_user,
                Some(&pull_request_state),
            ));
            reconciliation.review_notifications_updated += 1;
        }

        Ok(reconciliation)
    }

    fn mark_review_notification_for_head(
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
        self.dispatch_repository.save_dispatch(&updated_record)
    }
}

pub(crate) fn first_follow_up_line(follow_up_request: &str) -> String {
    follow_up_request
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Continue the previous remote task.")
        .to_owned()
}

pub(crate) fn select_follow_up_base_dispatch(
    dispatch_history: &[TaskDispatchRecord],
) -> Option<TaskDispatchRecord> {
    dispatch_history
        .iter()
        .find(|record| {
            !record.status.is_active()
                && record.branch_name.is_some()
                && record.worktree_path.is_some()
        })
        .cloned()
}

pub(crate) fn select_previous_submitted_review_run<'a>(
    dispatch_history: &'a [ReviewRunRecord],
    current_dispatch_id: &str,
) -> Option<&'a ReviewRunRecord> {
    dispatch_history.iter().find(|record| {
        record.dispatch_id != current_dispatch_id
            && record.review_submitted
            && (record.github_review_url.is_some() || record.github_review_id.is_some())
    })
}

pub(crate) fn latest_pull_request_for_branch(
    dispatch_history: &[TaskDispatchRecord],
    branch_name: &str,
) -> Option<String> {
    dispatch_history
        .iter()
        .find(|record| {
            record.branch_name.as_deref() == Some(branch_name)
                && record
                    .pull_request_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_some()
        })
        .and_then(|record| record.pull_request_url.clone())
}
