use std::sync::Arc;

use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::database::DatabaseContext;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::{DispatchId, ReviewId, TaskId};
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath};
use track_types::time_utils::now_utc;
use track_types::types::{CreateReviewInput, DispatchStatus, ReviewRecord, ReviewRunRecord};

use crate::RemoteWorkspace;

pub(crate) use self::guard::ReviewDispatchStartGuard;

mod cancel;
mod guard;
mod launch;
mod refresh;

pub struct RemoteReviewService<'a> {
    pub(super) database: &'a DatabaseContext,
    pub(super) workspace: Arc<RemoteWorkspace>,
}

impl<'a> RemoteReviewService<'a> {
    fn project_repository(&self) -> ProjectRepository<'a> {
        self.database.project_repository()
    }

    fn review_repository(&self) -> ReviewRepository<'a> {
        self.database.review_repository()
    }

    fn review_dispatch_repository(&self) -> ReviewDispatchRepository<'a> {
        self.database.review_dispatch_repository()
    }

    // =============================================================================
    // Review Request Entry Points
    // =============================================================================
    //
    // This file mirrors the task dispatch service on purpose so the two
    // domains can be compared directly. Lifecycle adapter code lives in
    // sibling modules with matching names.
    #[tracing::instrument(skip(self, input), fields(preferred_tool = ?input.preferred_tool))]
    pub async fn create_review(
        &self,
        input: CreateReviewInput,
    ) -> Result<(ReviewRecord, ReviewRunRecord), TrackError> {
        let validated_input = input.validate();
        let review_settings = self.load_review_runtime_prerequisites().await?;
        let remote_agent = self.workspace.remote_agent();
        let pull_request_metadata = self
            .workspace
            .projects()
            .fetch_pull_request_metadata(&validated_input.pull_request_url)
            .await?;
        let initial_target_head_oid = pull_request_metadata.head_oid.clone();
        let project_match = self
            .project_repository()
            .list_projects()
            .await?
            .into_iter()
            .find(|project| project.metadata.repo_url == pull_request_metadata.repo_url);
        let project_metadata_override = project_match
            .as_ref()
            .map(|project| project.metadata.clone());
        let workspace_key = project_match
            .as_ref()
            .map(|project| project.canonical_name.as_workspace_key())
            .unwrap_or_else(|| pull_request_metadata.workspace_key.clone());
        let review_timestamp = now_utc();
        let mut review_id = ReviewId::new(
            TaskId::unique(
                review_timestamp,
                &format!(
                    "review {} pr {}",
                    pull_request_metadata.repository_full_name,
                    pull_request_metadata.pull_request_number
                ),
            )
            .as_str(),
        )
        .expect("generated review ids should be valid path components");

        // TODO: Do we need that?
        if self
            .review_repository()
            .get_review(&review_id)
            .await
            .is_ok()
        {
            let mut suffix = 2;
            loop {
                let candidate = ReviewId::new(format!("{review_id}-{suffix}"))
                    .expect("generated review ids should be valid path components");
                if self
                    .review_repository()
                    .get_review(&candidate)
                    .await
                    .is_err()
                {
                    review_id = candidate;
                    break;
                }
                suffix += 1;
            }
        }

        let review = ReviewRecord {
            id: review_id,
            pull_request_url: pull_request_metadata.pull_request_url,
            pull_request_number: pull_request_metadata.pull_request_number,
            pull_request_title: pull_request_metadata.pull_request_title,
            repository_full_name: pull_request_metadata.repository_full_name,
            repo_url: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.repo_url.clone())
                .unwrap_or(pull_request_metadata.repo_url),
            git_url: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.git_url.clone())
                .unwrap_or(pull_request_metadata.git_url),
            base_branch: project_metadata_override
                .as_ref()
                .map(|metadata| metadata.base_branch.clone())
                .unwrap_or(pull_request_metadata.base_branch),
            workspace_key,
            preferred_tool: validated_input
                .preferred_tool
                .unwrap_or(remote_agent.preferred_tool),
            project: project_match.map(|project| project.canonical_name),
            main_user: review_settings.main_user,
            default_review_prompt: review_settings.default_review_prompt,
            extra_instructions: validated_input.extra_instructions,
            created_at: review_timestamp,
            updated_at: review_timestamp,
        };

        self.review_repository().save_review(&review).await?;
        match self
            .queue_review_dispatch(
                &review,
                remote_agent,
                None,
                Some(initial_target_head_oid.as_str()),
            )
            .await
        {
            Ok(dispatch) => {
                tracing::info!(
                    review_id = %review.id,
                    dispatch_id = %dispatch.run.dispatch_id,
                    remote_host = %dispatch.run.remote_host,
                    preferred_tool = ?dispatch.run.preferred_tool,
                    "Created PR review and queued initial remote run"
                );
                Ok((review, dispatch))
            }
            Err(error) => {
                let _ = self.review_repository().delete_review(&review.id).await;
                Err(error)
            }
        }
    }

    // =============================================================================
    // Follow-Up Review Runs
    // =============================================================================
    //
    // A re-review should feel like the PR equivalent of a task follow-up: the
    // saved review record remains the durable anchor, while each new run stores
    // the latest user ask plus the exact PR head it targeted. We deliberately
    // fetch fresh PR metadata here so each run records which commit the agent
    // reviewed instead of assuming the PR stayed on the same head as the
    // initial request.
    #[tracing::instrument(skip(self, follow_up_request), fields(review_id = %review_id))]
    pub async fn queue_follow_up_review_dispatch(
        &self,
        review_id: &ReviewId,
        follow_up_request: &str,
    ) -> Result<ReviewRunRecord, TrackError> {
        let trimmed_follow_up_request = follow_up_request.trim();
        if trimmed_follow_up_request.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a re-review request for the remote agent.",
            ));
        }

        let mut review = self.load_review_dispatch_prerequisites(review_id).await?;
        let _dispatch_start_guard = ReviewDispatchStartGuard::acquire(review_id);
        self.ensure_no_blocking_active_review_dispatch(review_id)
            .await?;

        let remote_agent = self.workspace.remote_agent();
        let pull_request_metadata = self
            .workspace
            .projects()
            .fetch_pull_request_metadata(&review.pull_request_url)
            .await?;
        let previous_updated_at = review.updated_at;
        review.updated_at = now_utc();
        self.review_repository().save_review(&review).await?;

        match self
            .queue_review_dispatch(
                &review,
                remote_agent,
                Some(trimmed_follow_up_request),
                Some(pull_request_metadata.head_oid.as_str()),
            )
            .await
        {
            Ok(dispatch) => {
                tracing::info!(
                    dispatch_id = %dispatch.run.dispatch_id,
                    remote_host = %dispatch.run.remote_host,
                    preferred_tool = ?dispatch.run.preferred_tool,
                    follow_up_lines = trimmed_follow_up_request.lines().count(),
                    "Queued PR review follow-up run"
                );
                Ok(dispatch)
            }
            Err(error) => {
                review.updated_at = previous_updated_at;
                let _ = self.review_repository().save_review(&review).await;
                Err(error)
            }
        }
    }

    #[tracing::instrument(
        skip(self, dispatch_record),
        fields(
            review_id = %dispatch_record.review_id,
            dispatch_id = %dispatch_record.run.dispatch_id,
            remote_host = %dispatch_record.run.remote_host,
            preferred_tool = ?dispatch_record.run.preferred_tool
        )
    )]
    pub async fn launch_prepared_review(
        &self,
        dispatch_record: ReviewRunRecord,
    ) -> Result<ReviewRunRecord, TrackError> {
        launch::launch_prepared_review(self, dispatch_record).await
    }

    pub async fn latest_dispatches_for_reviews(
        &self,
        review_ids: &[ReviewId],
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self
            .review_dispatch_repository()
            .latest_dispatches_for_reviews(review_ids)
            .await?;
        self.refresh_active_review_dispatch_records(records).await
    }

    pub async fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self
            .review_dispatch_repository()
            .list_dispatches(limit)
            .await?;
        self.refresh_active_review_dispatch_records(records).await
    }

    pub async fn cancel_dispatch(
        &self,
        review_id: &ReviewId,
    ) -> Result<ReviewRunRecord, TrackError> {
        cancel::cancel_dispatch(self, review_id).await
    }

    #[tracing::instrument(skip(self), fields(review_id = %review_id))]
    pub async fn delete_review(&self, review_id: &ReviewId) -> Result<(), TrackError> {
        let review = self.review_repository().get_review(review_id).await?;
        let dispatch_history = self
            .review_dispatch_repository()
            .dispatches_for_review(review_id)
            .await?;
        if !dispatch_history.is_empty() {
            if let Err(error) = self
                .cleanup_review_remote_artifacts(&review, &dispatch_history)
                .await
            {
                if !remote_cleanup_can_be_skipped(&error) {
                    return Err(error);
                }
                tracing::warn!(error = %error, "Skipping remote cleanup while deleting review");
            }

            self.review_dispatch_repository()
                .delete_dispatch_history_for_review(review_id)
                .await?;
        }

        self.review_repository().delete_review(review_id).await?;
        tracing::info!("Deleted review and any local run history");
        Ok(())
    }

    pub async fn refresh_active_review_dispatch_records(
        &self,
        records: Vec<ReviewRunRecord>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        refresh::refresh_active_review_dispatch_records(self, records).await
    }

    #[cfg(test)]
    pub(super) fn refresh_review_dispatch_record_from_snapshot(
        &self,
        record: ReviewRunRecord,
        snapshot: &crate::RemoteRunSnapshotView,
    ) -> Result<ReviewRunRecord, TrackError> {
        refresh::refresh_review_dispatch_record_from_snapshot(record, snapshot)
    }

    async fn ensure_no_blocking_active_review_dispatch(
        &self,
        review_id: &ReviewId,
    ) -> Result<(), TrackError> {
        if let Some(existing_dispatch) = self
            .latest_dispatches_for_reviews(std::slice::from_ref(review_id))
            .await?
            .into_iter()
            .next()
            .filter(|record| record.run.status.is_active())
        {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Review {review_id} already has an active remote run ({})",
                    existing_dispatch.run.dispatch_id
                ),
            ));
        }

        Ok(())
    }

    async fn queue_review_dispatch(
        &self,
        review: &ReviewRecord,
        remote_agent: &RemoteAgentRuntimeConfig,
        follow_up_request: Option<&str>,
        target_head_oid: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let dispatch_id = DispatchId::unique();
        let branch_name = DispatchBranch::for_review(&dispatch_id);
        let worktree_path = DispatchWorktreePath::for_review(
            &remote_agent.workspace_root,
            &review.workspace_key,
            &dispatch_id,
        );
        let follow_up_request = follow_up_request
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let target_head_oid = target_head_oid
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let summary = follow_up_request.map(|follow_up_request| {
            format!(
                "Re-review request: {}",
                first_follow_up_line(follow_up_request)
            )
        });
        let dispatch_record = self
            .review_dispatch_repository()
            .create_dispatch(
                review,
                &dispatch_id,
                &remote_agent.host,
                review.preferred_tool,
                &branch_name,
                &worktree_path,
                follow_up_request,
                target_head_oid,
                summary.as_deref(),
            )
            .await?;

        tracing::info!(
            review_id = %review.id,
            dispatch_id = %dispatch_record.run.dispatch_id,
            remote_host = %dispatch_record.run.remote_host,
            branch_name = ?branch_name,
            worktree_path = ?worktree_path,
            preferred_tool = ?dispatch_record.run.preferred_tool,
            has_follow_up_request = follow_up_request.is_some(),
            has_target_head_oid = target_head_oid.is_some(),
            "Queued remote PR review run"
        );

        Ok(dispatch_record)
    }

    async fn save_review_preparing_phase(
        &self,
        dispatch_record: &mut ReviewRunRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) = self
            .review_dispatch_repository()
            .get_dispatch(&dispatch_record.review_id, &dispatch_record.run.dispatch_id)
            .await?
        {
            if !saved_record.run.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
        } else {
            // If a user deletes the review or its local run history while the
            // background launcher is preparing the remote workspace, the
            // launch must stop instead of recreating the just-deleted row.
            return Ok(false);
        }

        *dispatch_record = dispatch_record.clone().into_preparing(summary);
        self.review_dispatch_repository()
            .save_dispatch(dispatch_record)
            .await?;
        tracing::info!(summary = %summary, "Updated review run preparation status");

        Ok(true)
    }

    async fn cancel_remote_review_if_possible(
        &self,
        dispatch_record: &ReviewRunRecord,
    ) -> Result<(), TrackError> {
        let Some(worktree_path) = dispatch_record.run.worktree_path.as_ref() else {
            return Ok(());
        };
        let _ = worktree_path;
        self.workspace
            .review_runs()
            .cancel(dispatch_record)
            .await
            .map(|_| ())?;
        tracing::info!(
            dispatch_id = %dispatch_record.run.dispatch_id,
            "Issued remote cancellation for review run"
        );
        Ok(())
    }

    async fn finalize_review_dispatch_locally(
        &self,
        dispatch_record: &ReviewRunRecord,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let updated_record =
            dispatch_record
                .clone()
                .into_locally_finalized(status, summary, error_message);
        self.review_dispatch_repository()
            .save_dispatch(&updated_record)
            .await?;
        if matches!(status, DispatchStatus::Blocked | DispatchStatus::Failed) {
            tracing::warn!(
                dispatch_id = %updated_record.run.dispatch_id,
                status = ?updated_record.run.status,
                summary = %summary,
                error_message = error_message.unwrap_or(""),
                "Locally finalized review run after remote disruption"
            );
        } else {
            tracing::info!(
                dispatch_id = %updated_record.run.dispatch_id,
                status = ?updated_record.run.status,
                summary = %summary,
                "Locally finalized review run"
            );
        }

        Ok(updated_record)
    }

    async fn cleanup_review_remote_artifacts(
        &self,
        review: &ReviewRecord,
        dispatch_history: &[ReviewRunRecord],
    ) -> Result<(), TrackError> {
        if dispatch_history.is_empty() {
            return Ok(());
        }

        self.workspace
            .review_runs()
            .cleanup(review, dispatch_history)
            .await
            .map(|_| ())?;
        tracing::info!(
            review_id = %review.id,
            remote_runs = dispatch_history.len(),
            "Cleaned remote PR review artifacts"
        );
        Ok(())
    }

    // =============================================================================
    // Review Runner Prerequisites
    // =============================================================================
    //
    // Saved reviews snapshot the review-specific knobs they need for future
    // re-reviews, namely the main GitHub user and default prompt. That means
    // later follow-up runs should only depend on the remote runner itself still
    // being usable, not on the mutable global review-follow-up block still
    // existing in the current config.
    fn ensure_review_runner_prerequisites(&self) -> Result<(), TrackError> {
        let remote_agent = self.workspace.remote_agent();

        if remote_agent
            .shell_prelude
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "Remote runner setup is missing. Open the web UI and add the shell instructions that prepare PATH and toolchains for the remote runner.",
            ));
        }

        Ok(())
    }

    async fn load_review_runtime_prerequisites(
        &self,
    ) -> Result<RemoteAgentReviewFollowUpRuntimeConfig, TrackError> {
        self.ensure_review_runner_prerequisites()?;
        let remote_agent = self.workspace.remote_agent();
        let review_settings = remote_agent.review_follow_up.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "PR reviews require a configured main GitHub user in the remote runner settings.",
            )
        })?;

        Ok(review_settings)
    }

    pub(super) async fn load_review_dispatch_prerequisites(
        &self,
        review_id: &ReviewId,
    ) -> Result<ReviewRecord, TrackError> {
        self.ensure_review_runner_prerequisites()?;
        let review = self.review_repository().get_review(review_id).await?;

        Ok(review)
    }
}

fn review_dispatch_not_found<'a>(
    review_id: &'a str,
    detail: &'a str,
) -> impl FnOnce() -> TrackError + 'a {
    move || {
        TrackError::new(
            ErrorCode::DispatchNotFound,
            format!("Review {review_id} {detail}"),
        )
    }
}

fn first_follow_up_line(follow_up_request: &str) -> String {
    follow_up_request
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Continue the previous remote review.")
        .to_owned()
}

fn remote_cleanup_can_be_skipped(error: &TrackError) -> bool {
    matches!(error.code, ErrorCode::RemoteDispatchFailed)
}

pub(super) fn select_previous_submitted_review_run<'a>(
    dispatch_history: &'a [ReviewRunRecord],
    current_dispatch_id: &DispatchId,
) -> Option<&'a ReviewRunRecord> {
    dispatch_history.iter().find(|record| {
        record.run.dispatch_id != *current_dispatch_id
            && record.review_submitted
            && (record.github_review_url.is_some() || record.github_review_id.is_some())
    })
}
