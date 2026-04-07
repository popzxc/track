use std::collections::{BTreeMap, BTreeSet};

use track_config::paths::collapse_home_path;
use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::database::DatabaseContext;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::{DispatchId, ReviewId, TaskId};
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath};
use track_types::time_utils::now_utc;
use track_types::types::{
    CreateReviewInput, DispatchStatus, RemoteAgentReviewOutcome, ReviewRecord, ReviewRunRecord,
};

use crate::constants::{PREPARING_STALE_AFTER, REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME};
use crate::prompts::RemoteReviewPrompt;
use crate::remote_actions::FetchPullRequestMetadataAction;
use crate::schemas::RemoteReviewSchema;
use crate::ssh::SshClient;
use crate::types::{ClaudeStructuredOutputEnvelope, RemoteDispatchSnapshot};
use crate::utils::{unique_review_run_directories, unique_review_worktree_paths};

use super::remote_agent_services::{
    load_refresh_ssh_client, RefreshRemoteClient, RemoteAgentConfigProvider, RemoteRunOps,
    RemoteWorkspaceOps,
};

pub(crate) use self::guard::ReviewDispatchStartGuard;

mod guard;

pub struct RemoteReviewService<'a> {
    pub(super) config_service: &'a dyn RemoteAgentConfigProvider,
    pub(super) database: &'a DatabaseContext,
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
    // This file mirrors `dispatch.rs` on purpose so the two domains can be
    // compared directly. Review-specific helpers stay inline here until the
    // service boundaries feel settled enough to split again with confidence.
    pub async fn create_review(
        &self,
        input: CreateReviewInput,
    ) -> Result<(ReviewRecord, ReviewRunRecord), TrackError> {
        let validated_input = input.validate();
        let (remote_agent, review_settings) = self.load_review_runtime_prerequisites().await?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            FetchPullRequestMetadataAction::new(&ssh_client, &validated_input.pull_request_url)
                .execute()?;
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
            .unwrap_or_else(|| pull_request_metadata.workspace_key());
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
                // TODO: Why the hell we `unwrap_or(false)` here?
                if !self
                    .review_repository()
                    .get_review(&candidate)
                    .await
                    .is_ok()
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
                &remote_agent,
                None,
                Some(initial_target_head_oid.as_str()),
            )
            .await
        {
            Ok(dispatch) => Ok((review, dispatch)),
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

        let (remote_agent, mut review) = self.load_review_dispatch_prerequisites(review_id).await?;
        let _dispatch_start_guard = ReviewDispatchStartGuard::acquire(review_id);
        self.ensure_no_blocking_active_review_dispatch(review_id)
            .await?;

        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            FetchPullRequestMetadataAction::new(&ssh_client, &review.pull_request_url).execute()?;
        let previous_updated_at = review.updated_at;
        review.updated_at = now_utc();
        self.review_repository().save_review(&review).await?;

        match self
            .queue_review_dispatch(
                &review,
                &remote_agent,
                Some(trimmed_follow_up_request),
                Some(pull_request_metadata.head_oid.as_str()),
            )
            .await
        {
            Ok(dispatch) => Ok(dispatch),
            Err(error) => {
                review.updated_at = previous_updated_at;
                let _ = self.review_repository().save_review(&review).await;
                Err(error)
            }
        }
    }

    pub async fn launch_prepared_review(
        &self,
        mut dispatch_record: ReviewRunRecord,
    ) -> Result<ReviewRunRecord, TrackError> {
        if let Some(existing_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)
            .await?
        {
            if !existing_record.status.is_active() {
                return Ok(existing_record);
            }
        }

        let worktree_path = dispatch_record
            .worktree_path
            .clone()
            .expect("queued review dispatches should store a worktree path");
        let branch_name = dispatch_record
            .branch_name
            .clone()
            .expect("queued review dispatches should store a branch name");
        let remote_run_directory = worktree_path.run_directory();

        let launch_result = async {
            if !self
                .save_review_preparing_phase(
                    &mut dispatch_record,
                    "Checking remote review prerequisites.",
                )
                .await?
            {
                return Ok::<(), TrackError>(());
            }
            let (remote_agent, review) = self
                .load_review_dispatch_prerequisites(&dispatch_record.review_id)
                .await?;
            let ssh_client = SshClient::new(&remote_agent)?;
            let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
            let runner = RemoteRunOps::new(&ssh_client);

            if !self
                .save_review_preparing_phase(
                    &mut dispatch_record,
                    "Ensuring the remote checkout is up to date.",
                )
                .await?
            {
                return Ok::<(), TrackError>(());
            }
            let checkout_path = workspace.ensure_review_checkout(&review)?;

            if !self
                .save_review_preparing_phase(&mut dispatch_record, "Preparing the review worktree.")
                .await?
            {
                return Ok::<(), TrackError>(());
            }
            workspace.prepare_review_worktree(
                &checkout_path,
                review.pull_request_number,
                &branch_name,
                &worktree_path,
                dispatch_record.target_head_oid.as_deref(),
            )?;

            let dispatch_history = self
                .review_dispatch_repository()
                .dispatches_for_review(&review.id)
                .await?;
            let previous_submitted_review = select_previous_submitted_review_run(
                &dispatch_history,
                &dispatch_record.dispatch_id,
            );
            let prompt =
                RemoteReviewPrompt::new(&review, &dispatch_record, previous_submitted_review)
                    .render();
            let schema = RemoteReviewSchema.render();
            if !self
                .save_review_preparing_phase(
                    &mut dispatch_record,
                    "Uploading the review prompt and schema.",
                )
                .await?
            {
                return Ok::<(), TrackError>(());
            }
            runner.upload_prompt_and_schema(
                &remote_run_directory.join(REMOTE_PROMPT_FILE_NAME),
                &prompt,
                &remote_run_directory.join(REMOTE_SCHEMA_FILE_NAME),
                &schema,
            )?;

            if !self
                .dispatch_is_still_active(&dispatch_record.review_id, &dispatch_record.dispatch_id)
                .await?
            {
                return Ok::<(), TrackError>(());
            }

            if !self
                .save_review_preparing_phase(
                    &mut dispatch_record,
                    "Launching the remote review agent.",
                )
                .await?
            {
                return Ok(());
            }
            runner.launch(
                &remote_run_directory,
                &worktree_path,
                dispatch_record.preferred_tool,
            )?;

            Ok(())
        }
        .await;

        match launch_result {
            Ok(()) => {
                if let Some(existing_record) = self
                    .load_saved_review_dispatch(
                        &dispatch_record.review_id,
                        &dispatch_record.dispatch_id,
                    )
                    .await?
                {
                    if !existing_record.status.is_active() {
                        let _ = self
                            .cancel_remote_review_if_possible(&existing_record)
                            .await;
                        return Ok(existing_record);
                    }
                }

                let dispatch_record = dispatch_record.into_running();
                self.review_dispatch_repository()
                    .save_dispatch(&dispatch_record)
                    .await?;
                Ok(dispatch_record)
            }
            Err(error) => {
                let dispatch_record = dispatch_record.into_failed(error.to_string());
                self.review_dispatch_repository()
                    .save_dispatch(&dispatch_record)
                    .await?;
                Err(error)
            }
        }
    }

    pub async fn latest_dispatches_for_reviews(
        &self,
        review_ids: &[ReviewId],
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = Vec::new();
        for review_id in review_ids {
            if let Some(record) = self
                .review_dispatch_repository()
                .latest_dispatch_for_review(review_id)
                .await?
            {
                records.push(record);
            }
        }

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

    pub async fn dispatch_history_for_review(
        &self,
        review_id: &ReviewId,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = self
            .review_dispatch_repository()
            .dispatches_for_review(review_id)
            .await?;
        if records
            .first()
            .is_some_and(|record| record.status.is_active())
        {
            if let Some(refreshed_latest) = self
                .latest_dispatches_for_reviews(std::slice::from_ref(review_id))
                .await?
                .into_iter()
                .next()
            {
                if let Some(first_record) = records.first_mut() {
                    *first_record = refreshed_latest;
                }
            }
        }

        Ok(records)
    }

    pub async fn cancel_dispatch(
        &self,
        review_id: &ReviewId,
    ) -> Result<ReviewRunRecord, TrackError> {
        let mut latest_dispatch = self
            .latest_dispatches_for_reviews(std::slice::from_ref(review_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(review_dispatch_not_found(
                review_id,
                "does not have a remote run to cancel.",
            ))?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Review {review_id} does not have an active remote run to cancel."),
            ));
        }

        self.cancel_remote_review_if_possible(&latest_dispatch)
            .await?;

        latest_dispatch = latest_dispatch.into_canceled_from_ui();
        self.review_dispatch_repository()
            .save_dispatch(&latest_dispatch)
            .await?;

        Ok(latest_dispatch)
    }

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
                eprintln!("Skipping remote cleanup while deleting review {review_id}: {error}");
            }

            self.review_dispatch_repository()
                .delete_dispatch_history_for_review(review_id)
                .await?;
        }

        self.review_repository().delete_review(review_id).await
    }

    async fn refresh_active_review_dispatch_records(
        &self,
        records: Vec<ReviewRunRecord>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let ssh_client = match load_refresh_ssh_client(self.config_service).await? {
            RefreshRemoteClient::Available(ssh_client) => ssh_client,
            RefreshRemoteClient::UnavailableLocally { error_message } => {
                return self
                    .release_active_review_dispatches_after_reconciliation_loss(
                        records,
                        "Remote reconciliation is unavailable locally, so active review runs were released.",
                        &error_message,
                    )
                    .await;
            }
        };
        let snapshots_by_dispatch_id = load_review_snapshots_for_records(&ssh_client, &records)?;
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(record.dispatch_id.as_str()) else {
                if let Some(updated) = record
                    .clone()
                    .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
                {
                    self.review_dispatch_repository()
                        .save_dispatch(&updated)
                        .await?;
                    refreshed_records.push(updated);
                } else {
                    let updated = self.finalize_review_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        "Remote reconciliation could not find this review run anymore, so it was released locally.",
                        Some("Remote review snapshot is missing."),
                    )
                    .await?;
                    refreshed_records.push(updated);
                }
                continue;
            };

            match self.refresh_review_dispatch_record_from_snapshot(record.clone(), snapshot) {
                Ok(updated) => {
                    if updated != record {
                        self.review_dispatch_repository()
                            .save_dispatch(&updated)
                            .await?;
                    }
                    refreshed_records.push(updated);
                }
                Err(error) => {
                    if snapshot.is_finished() {
                        let refreshed_at = now_utc();
                        let finished_at = snapshot.finished_at_or(refreshed_at);
                        let updated = record.clone().mark_failed_from_remote_refresh(
                            refreshed_at,
                            finished_at,
                            error.to_string(),
                        );
                        self.review_dispatch_repository()
                            .save_dispatch(&updated)
                            .await?;
                        refreshed_records.push(updated);
                    } else {
                        let error_message = error.to_string();
                        let updated = self.finalize_review_dispatch_locally(
                            &record,
                            DispatchStatus::Blocked,
                            "Remote reconciliation could not confirm this review run, so it was released locally.",
                            Some(&error_message),
                        )
                        .await?;
                        refreshed_records.push(updated);
                    }
                }
            }
        }

        Ok(refreshed_records)
    }

    pub(super) fn refresh_review_dispatch_record_from_snapshot(
        &self,
        record: ReviewRunRecord,
        snapshot: &RemoteDispatchSnapshot,
    ) -> Result<ReviewRunRecord, TrackError> {
        if snapshot.is_missing() {
            if let Some(updated) = record
                .clone()
                .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
            {
                return Ok(updated);
            }

            return Ok(record);
        }

        if snapshot.is_running() {
            return Ok(record.mark_running_from_remote(now_utc()));
        }

        if snapshot.is_canceled() {
            let refreshed_at = now_utc();
            let finished_at = snapshot.finished_at_or(refreshed_at);
            return Ok(record.mark_canceled_from_remote(refreshed_at, finished_at));
        }

        let refreshed_at = now_utc();
        let finished_at = snapshot.finished_at_or(refreshed_at);
        if snapshot.is_completed() {
            let remote_result = snapshot.required_result(
                "Remote review run completed without producing a structured result.",
            )?;
            let outcome = ClaudeStructuredOutputEnvelope::<RemoteAgentReviewOutcome>::parse_result(
                remote_result,
                record.preferred_tool,
                "Remote review result",
            )?;
            return Ok(record.apply_remote_review_outcome(outcome, refreshed_at, finished_at));
        }

        Ok(record.mark_failed_from_remote_refresh(
            refreshed_at,
            finished_at,
            snapshot
                .failure_message("Remote review run failed before returning a structured result."),
        ))
    }

    async fn release_active_review_dispatches_after_reconciliation_loss(
        &self,
        records: Vec<ReviewRunRecord>,
        summary: &str,
        error_message: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if record.status.is_active() {
                refreshed_records.push(
                    self.finalize_review_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        summary,
                        Some(error_message),
                    )
                    .await?,
                );
            } else {
                refreshed_records.push(record);
            }
        }

        Ok(refreshed_records)
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
            .filter(|record| record.status.is_active())
        {
            return Err(TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                format!(
                    "Review {review_id} already has an active remote run ({})",
                    existing_dispatch.dispatch_id
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

        Ok(dispatch_record)
    }

    async fn load_saved_review_dispatch(
        &self,
        review_id: &ReviewId,
        dispatch_id: &DispatchId,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        self.review_dispatch_repository()
            .get_dispatch(review_id, dispatch_id)
            .await
    }

    async fn dispatch_is_still_active(
        &self,
        review_id: &ReviewId,
        dispatch_id: &DispatchId,
    ) -> Result<bool, TrackError> {
        Ok(self
            .load_saved_review_dispatch(review_id, dispatch_id)
            .await?
            .map(|record| record.status.is_active())
            .unwrap_or(false))
    }

    async fn save_review_preparing_phase(
        &self,
        dispatch_record: &mut ReviewRunRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)
            .await?
        {
            if !saved_record.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
        }

        *dispatch_record = dispatch_record.clone().into_preparing(summary);
        self.review_dispatch_repository()
            .save_dispatch(dispatch_record)
            .await?;

        Ok(true)
    }

    async fn cancel_remote_review_if_possible(
        &self,
        dispatch_record: &ReviewRunRecord,
    ) -> Result<(), TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()
            .await?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        let Some(worktree_path) = dispatch_record.worktree_path.as_ref() else {
            return Ok(());
        };
        let remote_run_directory = worktree_path.run_directory();
        let ssh_client = SshClient::new(&remote_agent)?;
        RemoteRunOps::new(&ssh_client).cancel(&remote_run_directory)
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

        let remote_agent = self
            .load_remote_agent_for_review_cleanup(&review.id)
            .await?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
        let checkout_path = workspace.resolve_checkout_path(&review.workspace_key)?;
        let worktree_paths = unique_review_worktree_paths(dispatch_history);
        let run_directories = unique_review_run_directories(dispatch_history, &remote_agent);
        let branch_names = dispatch_history
            .iter()
            .filter_map(|record| record.branch_name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        workspace.cleanup_review_artifacts(
            &checkout_path,
            &branch_names,
            &worktree_paths,
            &run_directories,
        )
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
    async fn load_review_runner_prerequisites(
        &self,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()
            .await?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    "Remote reviews are not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

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

        Ok(remote_agent)
    }

    async fn load_review_runtime_prerequisites(
        &self,
    ) -> Result<
        (
            RemoteAgentRuntimeConfig,
            RemoteAgentReviewFollowUpRuntimeConfig,
        ),
        TrackError,
    > {
        let remote_agent = self.load_review_runner_prerequisites().await?;
        let review_settings = remote_agent.review_follow_up.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "PR reviews require a configured main GitHub user in the remote runner settings.",
            )
        })?;

        Ok((remote_agent, review_settings))
    }

    pub(super) async fn load_review_dispatch_prerequisites(
        &self,
        review_id: &ReviewId,
    ) -> Result<(RemoteAgentRuntimeConfig, ReviewRecord), TrackError> {
        let remote_agent = self.load_review_runner_prerequisites().await?;
        let review = self.review_repository().get_review(review_id).await?;

        Ok((remote_agent, review))
    }

    async fn load_remote_agent_for_review_cleanup(
        &self,
        review_id: &ReviewId,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()
            .await?
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::RemoteAgentNotConfigured,
                    format!(
                        "Review {review_id} has remote history, but remote-agent configuration is missing so cleanup cannot run."
                    ),
                )
            })?;

        if !remote_agent.managed_key_path.exists() {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                format!(
                    "Managed SSH key not found at {}. Re-run `track` and import the remote-agent key again before cleaning review {review_id}.",
                    collapse_home_path(&remote_agent.managed_key_path)
                ),
            ));
        }

        Ok(remote_agent)
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

pub(super) fn select_previous_submitted_review_run<'a>(
    dispatch_history: &'a [ReviewRunRecord],
    current_dispatch_id: &DispatchId,
) -> Option<&'a ReviewRunRecord> {
    dispatch_history.iter().find(|record| {
        record.dispatch_id != *current_dispatch_id
            && record.review_submitted
            && (record.github_review_url.is_some() || record.github_review_id.is_some())
    })
}

fn load_review_snapshots_for_records(
    ssh_client: &SshClient,
    records: &[ReviewRunRecord],
) -> Result<BTreeMap<String, RemoteDispatchSnapshot>, TrackError> {
    let mut dispatch_ids = Vec::new();
    let mut run_directories = Vec::new();

    for record in records {
        if !record.status.is_active() {
            continue;
        }

        let Some(worktree_path) = record.worktree_path.as_ref() else {
            continue;
        };

        dispatch_ids.push(record.dispatch_id.to_string());
        run_directories.push(worktree_path.run_directory());
    }

    if run_directories.is_empty() {
        return Ok(BTreeMap::new());
    }

    let snapshots = RemoteRunOps::new(ssh_client).read_snapshots(&run_directories)?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}
