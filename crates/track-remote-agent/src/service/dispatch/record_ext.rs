use track_types::time_utils::now_utc;
use track_types::types::{DispatchStatus, TaskDispatchRecord};

// =============================================================================
// Task Dispatch Record Transitions
// =============================================================================
//
// `TaskDispatchRecord` lives in shared types, but this file owns the dispatch-
// specific meaning of "queued", "follow-up", "preparing", and similar states.
// A local extension trait keeps those transitions readable without pretending
// the shared type should expose these domain rules everywhere.
pub(super) trait TaskDispatchRecordExt {
    fn into_preparing(self, summary: &str) -> Self;
    fn into_running(self) -> Self;
    fn into_failed(self, error_message: String) -> Self;
    fn into_canceled_from_ui(self) -> Self;
    fn into_locally_finalized(
        self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self;
}

impl TaskDispatchRecordExt for TaskDispatchRecord {
    fn into_preparing(mut self, summary: &str) -> Self {
        self.status = DispatchStatus::Preparing;
        self.summary = Some(summary.to_owned());
        self.updated_at = now_utc();
        self.finished_at = None;
        self.error_message = None;
        self
    }

    fn into_running(mut self) -> Self {
        self.status = DispatchStatus::Running;
        self.updated_at = now_utc();
        self.finished_at = None;
        self.summary = Some("The remote agent is working in the prepared environment.".to_owned());
        self.error_message = None;
        self
    }

    fn into_failed(mut self, error_message: String) -> Self {
        self.status = DispatchStatus::Failed;
        self.updated_at = now_utc();
        self.finished_at = Some(self.updated_at);
        self.error_message = Some(error_message);
        self
    }

    fn into_canceled_from_ui(self) -> Self {
        self.into_locally_finalized(DispatchStatus::Canceled, "Canceled from the web UI.", None)
    }

    fn into_locally_finalized(
        mut self,
        status: DispatchStatus,
        summary: &str,
        error_message: Option<&str>,
    ) -> Self {
        let finished_at = now_utc();
        self.status = status;
        self.updated_at = finished_at;
        self.finished_at = Some(finished_at);
        self.summary = Some(summary.to_owned());
        self.notes = None;
        self.error_message = error_message.map(ToOwned::to_owned);
        self
    }
}

pub(super) fn first_follow_up_line(follow_up_request: &str) -> String {
    follow_up_request
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Continue the previous remote task.")
        .to_owned()
}
