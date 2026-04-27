use track_types::errors::TrackError;
use track_types::remote_layout::RemoteCheckoutPath;
use track_types::types::{RemoteRunState, ReviewRecord, ReviewRunRecord};

use super::super::lifecycle::launch::{
    launch_prepared_remote_run, RemoteRunLaunchAdapter, RemoteRunLaunchMessages,
};
use crate::prompts::RemoteReviewPrompt;
use crate::schemas::RemoteReviewSchema;

use super::{select_previous_submitted_review_run, RemoteReviewService};

pub(super) async fn launch_prepared_review(
    service: &RemoteReviewService<'_>,
    dispatch_record: ReviewRunRecord,
) -> Result<ReviewRunRecord, TrackError> {
    launch_prepared_remote_run(&ReviewRunLaunchAdapter { service }, dispatch_record).await
}

struct ReviewRunLaunchContext {
    review: ReviewRecord,
}

struct ReviewRunLaunchAdapter<'service, 'database> {
    service: &'service RemoteReviewService<'database>,
}

#[async_trait::async_trait]
impl RemoteRunLaunchAdapter for ReviewRunLaunchAdapter<'_, '_> {
    type Record = ReviewRunRecord;
    type Context = ReviewRunLaunchContext;

    fn messages(&self) -> RemoteRunLaunchMessages {
        RemoteRunLaunchMessages {
            run_kind: "review_run",
            skipped_inactive: "Skipped review launch because run is no longer active.",
            check_prerequisites_summary: "Checking remote review prerequisites.",
            stopped_before_prerequisites:
                "Review launch stopped before prerequisites because run is no longer active.",
            ensure_checkout_summary: "Ensuring the remote checkout is up to date.",
            stopped_before_checkout:
                "Review launch stopped while refreshing checkout because run is no longer active.",
            prepare_worktree_summary: "Preparing the review worktree.",
            stopped_before_worktree:
                "Review launch stopped while preparing worktree because run is no longer active.",
            upload_files_summary: "Uploading the review prompt and schema.",
            stopped_before_upload:
                "Review launch stopped while uploading run files because run is no longer active.",
            canceled_during_preparation:
                "Review launch stopped because run was canceled during preparation.",
            launch_remote_summary: "Launching the remote review agent.",
            stopped_before_launch:
                "Review launch stopped before remote agent start because run is no longer active.",
            changed_before_promotion:
                "Review run changed state before promotion; returning persisted state.",
            marked_running: "Marked review run as running.",
            launch_failed: "Remote review launch failed.",
        }
    }

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
        &record.run
    }

    async fn load_saved_record(
        &self,
        record: &Self::Record,
    ) -> Result<Option<Self::Record>, TrackError> {
        self.service
            .review_dispatch_repository()
            .get_dispatch(&record.review_id, &record.run.dispatch_id)
            .await
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .review_dispatch_repository()
            .save_dispatch(record)
            .await
    }

    async fn save_preparing_phase(
        &self,
        record: &mut Self::Record,
        summary: &str,
    ) -> Result<bool, TrackError> {
        self.service
            .save_review_preparing_phase(record, summary)
            .await
    }

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service.cancel_remote_review_if_possible(record).await
    }

    fn into_running(&self, record: Self::Record) -> Self::Record {
        record.into_running()
    }

    fn into_failed(&self, record: Self::Record, error_message: String) -> Self::Record {
        record.into_failed(error_message)
    }

    async fn load_context(&self, record: &Self::Record) -> Result<Self::Context, TrackError> {
        let worktree_path = record
            .run
            .worktree_path
            .clone()
            .expect("queued review dispatches should store a worktree path");
        record
            .run
            .branch_name
            .clone()
            .expect("queued review dispatches should store a branch name");
        let _remote_run_directory = worktree_path.run_directory();
        let review = self
            .service
            .load_review_dispatch_prerequisites(&record.review_id)
            .await?;
        let remote_agent = self.service.workspace.remote_agent();
        tracing::info!(
            workspace_root = %remote_agent.workspace_root,
            pull_request_url = %review.pull_request_url,
            "Loaded PR review prerequisites"
        );

        Ok(ReviewRunLaunchContext { review })
    }

    async fn ensure_checkout(
        &self,
        _record: &Self::Record,
        context: &Self::Context,
    ) -> Result<RemoteCheckoutPath, TrackError> {
        let checkout_path = self
            .service
            .workspace
            .projects()
            .ensure_review_checkout(&context.review)
            .await?;
        tracing::info!(checkout_path = ?checkout_path, "Remote review checkout is ready");

        Ok(checkout_path)
    }

    async fn prepare_worktree(
        &self,
        record: &Self::Record,
        context: &Self::Context,
        checkout_path: &RemoteCheckoutPath,
    ) -> Result<(), TrackError> {
        self.service
            .workspace
            .review_runs()
            .prepare_worktree(
                record,
                checkout_path,
                context.review.pull_request_number,
                record.target_head_oid.as_deref(),
            )
            .await?;
        tracing::info!("Prepared remote review worktree");

        Ok(())
    }

    async fn upload_run_files(
        &self,
        record: &Self::Record,
        context: &Self::Context,
    ) -> Result<(), TrackError> {
        let dispatch_history = self
            .service
            .review_dispatch_repository()
            .dispatches_for_review(&context.review.id)
            .await?;
        let previous_submitted_review =
            select_previous_submitted_review_run(&dispatch_history, &record.run.dispatch_id);
        let prompt =
            RemoteReviewPrompt::new(&context.review, record, previous_submitted_review).render();
        let schema = RemoteReviewSchema.render();
        self.service
            .workspace
            .review_runs()
            .upload_run_files(record, &prompt, &schema)
            .await?;
        tracing::info!("Uploaded remote review prompt and schema");

        Ok(())
    }

    async fn launch_remote(
        &self,
        record: &Self::Record,
        _context: &Self::Context,
    ) -> Result<(), TrackError> {
        self.service.workspace.review_runs().launch(record).await?;
        tracing::info!("Started remote review agent process");

        Ok(())
    }
}
