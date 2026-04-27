use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::ReviewId;
use track_types::types::{RemoteRunState, ReviewRunRecord};

use super::super::lifecycle::cancel::{
    cancel_latest_remote_run, RemoteRunCancelAdapter, RemoteRunCancelMessages,
};

use super::{review_dispatch_not_found, RemoteReviewService};

pub(super) async fn cancel_dispatch(
    service: &RemoteReviewService<'_>,
    review_id: &ReviewId,
) -> Result<ReviewRunRecord, TrackError> {
    cancel_latest_remote_run(&ReviewRunCancelAdapter { service, review_id }).await
}

struct ReviewRunCancelAdapter<'service, 'database, 'id> {
    service: &'service RemoteReviewService<'database>,
    review_id: &'id ReviewId,
}

#[async_trait::async_trait]
impl RemoteRunCancelAdapter for ReviewRunCancelAdapter<'_, '_, '_> {
    type Record = ReviewRunRecord;

    fn messages(&self) -> RemoteRunCancelMessages {
        RemoteRunCancelMessages {
            run_kind: "review_run",
            canceled: "Canceled PR review run.",
        }
    }

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
        &record.run
    }

    async fn load_latest_record(&self) -> Result<Option<Self::Record>, TrackError> {
        self.service
            .latest_dispatches_for_reviews(std::slice::from_ref(self.review_id))
            .await
            .map(|records| records.into_iter().next())
    }

    fn not_found_error(&self) -> TrackError {
        review_dispatch_not_found(self.review_id, "does not have a remote run to cancel.")()
    }

    fn inactive_error(&self) -> TrackError {
        TrackError::new(
            ErrorCode::DispatchNotFound,
            format!(
                "Review {} does not have an active remote run to cancel.",
                self.review_id
            ),
        )
    }

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service.cancel_remote_review_if_possible(record).await
    }

    fn mark_canceled_from_ui(&self, record: Self::Record) -> Self::Record {
        record.into_canceled_from_ui()
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .review_dispatch_repository()
            .save_dispatch(record)
            .await
    }
}
