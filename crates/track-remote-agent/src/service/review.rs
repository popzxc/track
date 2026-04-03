use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Condvar, Mutex, OnceLock};

use track_config::paths::collapse_home_path;
use track_config::runtime::{RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig};
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_types::errors::{ErrorCode, TrackError};
use track_types::task_id::build_unique_task_id;
use track_types::time_utils::now_utc;
use track_types::types::{
    CreateReviewInput, DispatchStatus, RemoteAgentReviewOutcome, ReviewRecord, ReviewRunRecord,
};

use crate::constants::{
    PREPARING_STALE_AFTER, REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME,
    REVIEW_RUN_DIRECTORY_NAME, REVIEW_WORKTREE_DIRECTORY_NAME,
};
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

pub struct RemoteReviewService<'a> {
    pub(in crate::service) config_service: &'a dyn RemoteAgentConfigProvider,
    pub(in crate::service) project_repository: &'a ProjectRepository,
    pub(in crate::service) review_repository: &'a ReviewRepository,
    pub(in crate::service) review_dispatch_repository: &'a ReviewDispatchRepository,
}

impl<'a> RemoteReviewService<'a> {
    // =============================================================================
    // Review Request Entry Points
    // =============================================================================
    //
    // This file mirrors `dispatch.rs` on purpose so the two domains can be
    // compared directly. Review-specific helpers stay inline here until the
    // service boundaries feel settled enough to split again with confidence.
    pub fn create_review(
        &self,
        input: CreateReviewInput,
    ) -> Result<(ReviewRecord, ReviewRunRecord), TrackError> {
        let validated_input = input.validate()?;
        let (remote_agent, review_settings) = self.load_review_runtime_prerequisites()?;
        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            FetchPullRequestMetadataAction::new(&ssh_client, &validated_input.pull_request_url)
                .execute()?;
        let initial_target_head_oid = pull_request_metadata.head_oid.clone();
        let project_match = self
            .project_repository
            .list_projects()?
            .into_iter()
            .find(|project| project.metadata.repo_url.trim() == pull_request_metadata.repo_url);
        let project_metadata_override = project_match
            .as_ref()
            .map(|project| project.metadata.clone());
        let workspace_key = project_match
            .as_ref()
            .map(|project| project.canonical_name.clone())
            .unwrap_or_else(|| pull_request_metadata.workspace_key());
        let review_timestamp = now_utc();
        let review_id = build_unique_task_id(
            review_timestamp,
            &format!(
                "review {} pr {}",
                pull_request_metadata.repository_full_name,
                pull_request_metadata.pull_request_number
            ),
            |candidate| self.review_repository.get_review(candidate).is_ok(),
        );
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

        self.review_repository.save_review(&review)?;
        match self.queue_review_dispatch(
            &review,
            &remote_agent,
            None,
            Some(initial_target_head_oid.as_str()),
        ) {
            Ok(dispatch) => Ok((review, dispatch)),
            Err(error) => {
                let _ = self.review_repository.delete_review(&review.id);
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
    pub fn queue_follow_up_review_dispatch(
        &self,
        review_id: &str,
        follow_up_request: &str,
    ) -> Result<ReviewRunRecord, TrackError> {
        let trimmed_follow_up_request = follow_up_request.trim();
        if trimmed_follow_up_request.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a re-review request for the remote agent.",
            ));
        }

        let (remote_agent, mut review) = self.load_review_dispatch_prerequisites(review_id)?;
        let _dispatch_start_guard = ReviewDispatchStartGuard::acquire(review_id);
        self.ensure_no_blocking_active_review_dispatch(review_id)?;

        let ssh_client = SshClient::new(&remote_agent)?;
        let pull_request_metadata =
            FetchPullRequestMetadataAction::new(&ssh_client, &review.pull_request_url).execute()?;
        let previous_updated_at = review.updated_at;
        review.updated_at = now_utc();
        self.review_repository.save_review(&review)?;

        match self.queue_review_dispatch(
            &review,
            &remote_agent,
            Some(trimmed_follow_up_request),
            Some(pull_request_metadata.head_oid.as_str()),
        ) {
            Ok(dispatch) => Ok(dispatch),
            Err(error) => {
                review.updated_at = previous_updated_at;
                let _ = self.review_repository.save_review(&review);
                Err(error)
            }
        }
    }

    pub fn launch_prepared_review(
        &self,
        mut dispatch_record: ReviewRunRecord,
    ) -> Result<ReviewRunRecord, TrackError> {
        if let Some(existing_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)?
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
        let remote_run_directory =
            derive_review_run_directory(&worktree_path, &dispatch_record.dispatch_id)?;

        let launch_result = (|| -> Result<(), TrackError> {
            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Checking remote review prerequisites.",
            )? {
                return Ok(());
            }
            let (remote_agent, review) =
                self.load_review_dispatch_prerequisites(&dispatch_record.review_id)?;
            let ssh_client = SshClient::new(&remote_agent)?;
            let workspace = RemoteWorkspaceOps::new(&ssh_client, &remote_agent);
            let runner = RemoteRunOps::new(&ssh_client);

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Ensuring the remote checkout is up to date.",
            )? {
                return Ok(());
            }
            let checkout_path = workspace.ensure_review_checkout(&review)?;

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Preparing the review worktree.",
            )? {
                return Ok(());
            }
            workspace.prepare_review_worktree(
                &checkout_path,
                review.pull_request_number,
                &branch_name,
                &worktree_path,
                dispatch_record.target_head_oid.as_deref(),
            )?;

            let dispatch_history = self
                .review_dispatch_repository
                .dispatches_for_review(&review.id)?;
            let previous_submitted_review = select_previous_submitted_review_run(
                &dispatch_history,
                &dispatch_record.dispatch_id,
            );
            let prompt =
                RemoteReviewPrompt::new(&review, &dispatch_record, previous_submitted_review)
                    .render();
            let schema = RemoteReviewSchema.render();
            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Uploading the review prompt and schema.",
            )? {
                return Ok(());
            }
            runner.upload_prompt_and_schema(
                &format!("{remote_run_directory}/{REMOTE_PROMPT_FILE_NAME}"),
                &prompt,
                &format!("{remote_run_directory}/{REMOTE_SCHEMA_FILE_NAME}"),
                &schema,
            )?;

            if !self.dispatch_is_still_active(
                &dispatch_record.review_id,
                &dispatch_record.dispatch_id,
            )? {
                return Ok(());
            }

            if !self.save_review_preparing_phase(
                &mut dispatch_record,
                "Launching the remote review agent.",
            )? {
                return Ok(());
            }
            runner.launch(
                &remote_run_directory,
                &worktree_path,
                dispatch_record.preferred_tool,
            )?;

            Ok(())
        })();

        match launch_result {
            Ok(()) => {
                if let Some(existing_record) = self.load_saved_review_dispatch(
                    &dispatch_record.review_id,
                    &dispatch_record.dispatch_id,
                )? {
                    if !existing_record.status.is_active() {
                        let _ = self.cancel_remote_review_if_possible(&existing_record);
                        return Ok(existing_record);
                    }
                }

                dispatch_record.status = DispatchStatus::Running;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = None;
                dispatch_record.summary =
                    Some("The remote agent is reviewing the prepared pull request.".to_owned());
                dispatch_record.error_message = None;
                self.review_dispatch_repository
                    .save_dispatch(&dispatch_record)?;
                Ok(dispatch_record)
            }
            Err(error) => {
                dispatch_record.status = DispatchStatus::Failed;
                dispatch_record.updated_at = now_utc();
                dispatch_record.finished_at = Some(dispatch_record.updated_at);
                dispatch_record.error_message = Some(error.to_string());
                self.review_dispatch_repository
                    .save_dispatch(&dispatch_record)?;
                Err(error)
            }
        }
    }

    pub fn latest_dispatches_for_reviews(
        &self,
        review_ids: &[String],
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = Vec::new();
        for review_id in review_ids {
            if let Some(record) = self
                .review_dispatch_repository
                .latest_dispatch_for_review(review_id)?
            {
                records.push(record);
            }
        }

        self.refresh_active_review_dispatch_records(records)
    }

    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let records = self.review_dispatch_repository.list_dispatches(limit)?;
        self.refresh_active_review_dispatch_records(records)
    }

    pub fn dispatch_history_for_review(
        &self,
        review_id: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut records = self
            .review_dispatch_repository
            .dispatches_for_review(review_id)?;
        if records
            .first()
            .is_some_and(|record| record.status.is_active())
        {
            if let Some(refreshed_latest) = self
                .latest_dispatches_for_reviews(&[review_id.to_owned()])?
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

    pub fn cancel_dispatch(&self, review_id: &str) -> Result<ReviewRunRecord, TrackError> {
        let mut latest_dispatch = self
            .latest_dispatches_for_reviews(&[review_id.to_owned()])?
            .into_iter()
            .next()
            .ok_or_else(|| {
                TrackError::new(
                    ErrorCode::DispatchNotFound,
                    format!("Review {review_id} does not have a remote run to cancel."),
                )
            })?;

        if !latest_dispatch.status.is_active() {
            return Err(TrackError::new(
                ErrorCode::DispatchNotFound,
                format!("Review {review_id} does not have an active remote run to cancel."),
            ));
        }

        self.cancel_remote_review_if_possible(&latest_dispatch)?;

        latest_dispatch.status = DispatchStatus::Canceled;
        latest_dispatch.updated_at = now_utc();
        latest_dispatch.finished_at = Some(latest_dispatch.updated_at);
        latest_dispatch.summary = Some("Canceled from the web UI.".to_owned());
        latest_dispatch.notes = None;
        latest_dispatch.error_message = None;
        self.review_dispatch_repository
            .save_dispatch(&latest_dispatch)?;

        Ok(latest_dispatch)
    }

    pub fn delete_review(&self, review_id: &str) -> Result<(), TrackError> {
        let review = self.review_repository.get_review(review_id)?;
        let dispatch_history = self
            .review_dispatch_repository
            .dispatches_for_review(review_id)?;
        if !dispatch_history.is_empty() {
            if let Err(error) = self.cleanup_review_remote_artifacts(&review, &dispatch_history) {
                eprintln!("Skipping remote cleanup while deleting review {review_id}: {error}");
            }

            self.review_dispatch_repository
                .delete_dispatch_history_for_review(review_id)?;
        }

        self.review_repository.delete_review(review_id)
    }

    fn refresh_active_review_dispatch_records(
        &self,
        records: Vec<ReviewRunRecord>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let ssh_client = match load_refresh_ssh_client(self.config_service)? {
            RefreshRemoteClient::Available(ssh_client) => ssh_client,
            RefreshRemoteClient::UnavailableLocally { error_message } => {
                return self.release_active_review_dispatches_after_reconciliation_loss(
                    records,
                    "Remote reconciliation is unavailable locally, so active review runs were released.",
                    &error_message,
                );
            }
        };
        let snapshots_by_dispatch_id = load_review_snapshots_for_records(&ssh_client, &records)?;
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if !record.status.is_active() {
                refreshed_records.push(record);
                continue;
            }

            let Some(snapshot) = snapshots_by_dispatch_id.get(&record.dispatch_id) else {
                if let Some(updated) = record
                    .clone()
                    .mark_abandoned_if_preparing_stale(now_utc(), PREPARING_STALE_AFTER)
                {
                    self.review_dispatch_repository.save_dispatch(&updated)?;
                    refreshed_records.push(updated);
                } else {
                    let updated = self.finalize_review_dispatch_locally(
                        &record,
                        DispatchStatus::Blocked,
                        "Remote reconciliation could not find this review run anymore, so it was released locally.",
                        Some("Remote review snapshot is missing."),
                    )?;
                    refreshed_records.push(updated);
                }
                continue;
            };

            match self.refresh_review_dispatch_record_from_snapshot(record.clone(), snapshot) {
                Ok(updated) => {
                    if updated != record {
                        self.review_dispatch_repository.save_dispatch(&updated)?;
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
                        self.review_dispatch_repository.save_dispatch(&updated)?;
                        refreshed_records.push(updated);
                    } else {
                        let error_message = error.to_string();
                        let updated = self.finalize_review_dispatch_locally(
                            &record,
                            DispatchStatus::Blocked,
                            "Remote reconciliation could not confirm this review run, so it was released locally.",
                            Some(&error_message),
                        )?;
                        refreshed_records.push(updated);
                    }
                }
            }
        }

        Ok(refreshed_records)
    }

    pub(in crate::service) fn refresh_review_dispatch_record_from_snapshot(
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

    fn release_active_review_dispatches_after_reconciliation_loss(
        &self,
        records: Vec<ReviewRunRecord>,
        summary: &str,
        error_message: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let mut refreshed_records = Vec::with_capacity(records.len());
        for record in records {
            if record.status.is_active() {
                refreshed_records.push(self.finalize_review_dispatch_locally(
                    &record,
                    DispatchStatus::Blocked,
                    summary,
                    Some(error_message),
                )?);
            } else {
                refreshed_records.push(record);
            }
        }

        Ok(refreshed_records)
    }

    fn ensure_no_blocking_active_review_dispatch(&self, review_id: &str) -> Result<(), TrackError> {
        if let Some(existing_dispatch) = self
            .latest_dispatches_for_reviews(&[review_id.to_owned()])?
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

    fn queue_review_dispatch(
        &self,
        review: &ReviewRecord,
        remote_agent: &RemoteAgentRuntimeConfig,
        follow_up_request: Option<&str>,
        target_head_oid: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let mut dispatch_record = self.review_dispatch_repository.create_dispatch(
            review,
            &remote_agent.host,
            review.preferred_tool,
        )?;
        dispatch_record.branch_name = Some(format!("track-review/{}", dispatch_record.dispatch_id));
        dispatch_record.worktree_path = Some(format!(
            "{}/{}/{}/{}",
            remote_agent.workspace_root.trim_end_matches('/'),
            review.workspace_key,
            REVIEW_WORKTREE_DIRECTORY_NAME,
            dispatch_record.dispatch_id
        ));
        dispatch_record.follow_up_request = follow_up_request.map(str::trim).map(ToOwned::to_owned);
        dispatch_record.target_head_oid = target_head_oid
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(follow_up_request) = dispatch_record.follow_up_request.as_deref() {
            dispatch_record.summary = Some(format!(
                "Re-review request: {}",
                first_follow_up_line(follow_up_request)
            ));
        }
        dispatch_record.updated_at = now_utc();
        self.review_dispatch_repository
            .save_dispatch(&dispatch_record)?;

        Ok(dispatch_record)
    }

    fn load_saved_review_dispatch(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        self.review_dispatch_repository
            .get_dispatch(review_id, dispatch_id)
    }

    fn dispatch_is_still_active(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<bool, TrackError> {
        Ok(self
            .load_saved_review_dispatch(review_id, dispatch_id)?
            .map(|record| record.status.is_active())
            .unwrap_or(false))
    }

    fn save_review_preparing_phase(
        &self,
        dispatch_record: &mut ReviewRunRecord,
        summary: &str,
    ) -> Result<bool, TrackError> {
        if let Some(saved_record) = self
            .load_saved_review_dispatch(&dispatch_record.review_id, &dispatch_record.dispatch_id)?
        {
            if !saved_record.status.is_active() {
                *dispatch_record = saved_record;
                return Ok(false);
            }
        }

        dispatch_record.status = DispatchStatus::Preparing;
        dispatch_record.summary = Some(summary.to_owned());
        dispatch_record.updated_at = now_utc();
        dispatch_record.finished_at = None;
        dispatch_record.error_message = None;
        self.review_dispatch_repository
            .save_dispatch(dispatch_record)?;

        Ok(true)
    }

    fn cancel_remote_review_if_possible(
        &self,
        dispatch_record: &ReviewRunRecord,
    ) -> Result<(), TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
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

        let Some(worktree_path) = dispatch_record.worktree_path.as_deref() else {
            return Ok(());
        };
        let remote_run_directory =
            derive_review_run_directory(worktree_path, &dispatch_record.dispatch_id)?;
        let ssh_client = SshClient::new(&remote_agent)?;
        RemoteRunOps::new(&ssh_client).cancel(&remote_run_directory)
    }

    fn finalize_review_dispatch_locally(
        &self,
        dispatch_record: &ReviewRunRecord,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Result<ReviewRunRecord, TrackError> {
        let mut updated_record = dispatch_record.clone();
        let now = now_utc();
        updated_record.status = status;
        updated_record.updated_at = now;
        updated_record.finished_at = Some(now);
        updated_record.summary = Some(summary.to_owned());
        updated_record.notes = None;
        updated_record.error_message = error_message.map(ToOwned::to_owned);
        self.review_dispatch_repository
            .save_dispatch(&updated_record)?;

        Ok(updated_record)
    }

    fn cleanup_review_remote_artifacts(
        &self,
        review: &ReviewRecord,
        dispatch_history: &[ReviewRunRecord],
    ) -> Result<(), TrackError> {
        if dispatch_history.is_empty() {
            return Ok(());
        }

        let remote_agent = self.load_remote_agent_for_review_cleanup(&review.id)?;
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
    fn load_review_runner_prerequisites(&self) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
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

    fn load_review_runtime_prerequisites(
        &self,
    ) -> Result<
        (
            RemoteAgentRuntimeConfig,
            RemoteAgentReviewFollowUpRuntimeConfig,
        ),
        TrackError,
    > {
        let remote_agent = self.load_review_runner_prerequisites()?;
        let review_settings = remote_agent.review_follow_up.clone().ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidRemoteAgentConfig,
                "PR reviews require a configured main GitHub user in the remote runner settings.",
            )
        })?;

        Ok((remote_agent, review_settings))
    }

    pub(super) fn load_review_dispatch_prerequisites(
        &self,
        review_id: &str,
    ) -> Result<(RemoteAgentRuntimeConfig, ReviewRecord), TrackError> {
        let remote_agent = self.load_review_runner_prerequisites()?;
        let review = self.review_repository.get_review(review_id)?;

        Ok((remote_agent, review))
    }

    fn load_remote_agent_for_review_cleanup(
        &self,
        review_id: &str,
    ) -> Result<RemoteAgentRuntimeConfig, TrackError> {
        let remote_agent = self
            .config_service
            .load_remote_agent_runtime_config()?
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

fn first_follow_up_line(follow_up_request: &str) -> String {
    follow_up_request
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Continue the previous remote review.")
        .to_owned()
}

pub(in crate::service) fn select_previous_submitted_review_run<'a>(
    dispatch_history: &'a [ReviewRunRecord],
    current_dispatch_id: &str,
) -> Option<&'a ReviewRunRecord> {
    dispatch_history.iter().find(|record| {
        record.dispatch_id != current_dispatch_id
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

        let Some(worktree_path) = record.worktree_path.as_deref() else {
            continue;
        };
        let Ok(run_directory) = derive_review_run_directory(worktree_path, &record.dispatch_id)
        else {
            continue;
        };

        dispatch_ids.push(record.dispatch_id.clone());
        run_directories.push(run_directory);
    }

    if run_directories.is_empty() {
        return Ok(BTreeMap::new());
    }

    let snapshots = RemoteRunOps::new(ssh_client).read_snapshots(&run_directories)?;
    Ok(dispatch_ids.into_iter().zip(snapshots).collect())
}

fn derive_review_run_directory(
    worktree_path: &str,
    dispatch_id: &str,
) -> Result<String, TrackError> {
    worktree_path
        .rsplit_once(&format!("/{REVIEW_WORKTREE_DIRECTORY_NAME}/"))
        .map(|(prefix, _suffix)| format!("{prefix}/{REVIEW_RUN_DIRECTORY_NAME}/{dispatch_id}"))
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteDispatchFailed,
                "Could not derive the remote review run directory from the worktree path.",
            )
        })
}

#[derive(Debug, Default)]
struct ReviewDispatchStartGate {
    active_review_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
pub(crate) struct ReviewDispatchStartGuard {
    review_id: String,
}

impl ReviewDispatchStartGuard {
    pub(crate) fn acquire(review_id: &str) -> Self {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");

        while active_review_ids.contains(review_id) {
            active_review_ids = gate
                .wake_waiters
                .wait(active_review_ids)
                .expect("review dispatch start gate should not be poisoned");
        }

        active_review_ids.insert(review_id.to_owned());

        Self {
            review_id: review_id.to_owned(),
        }
    }
}

impl Drop for ReviewDispatchStartGuard {
    fn drop(&mut self) {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");
        active_review_ids.remove(&self.review_id);
        gate.wake_waiters.notify_all();
    }
}

fn review_dispatch_start_gate() -> &'static ReviewDispatchStartGate {
    static GATE: OnceLock<ReviewDispatchStartGate> = OnceLock::new();

    // Reviews are now follow-up capable, so the same "check for active work,
    // then persist a preparing record" race that tasks already guard against
    // applies here too. Keeping a dedicated gate per review id preserves the
    // review domain boundary without forcing the task flow to share review-only
    // coordination state.
    GATE.get_or_init(ReviewDispatchStartGate::default)
}
