use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::TaskId;
use track_types::types::{RemoteRunState, TaskDispatchRecord};

use super::super::lifecycle::cancel::{
    cancel_latest_remote_run, RemoteRunCancelAdapter, RemoteRunCancelMessages,
};

use super::{dispatch_not_found, RemoteDispatchService};

pub(super) async fn cancel_dispatch(
    service: &RemoteDispatchService<'_>,
    task_id: &TaskId,
) -> Result<TaskDispatchRecord, TrackError> {
    cancel_latest_remote_run(&TaskDispatchCancelAdapter { service, task_id }).await
}

struct TaskDispatchCancelAdapter<'service, 'database, 'id> {
    service: &'service RemoteDispatchService<'database>,
    task_id: &'id TaskId,
}

#[async_trait::async_trait]
impl RemoteRunCancelAdapter for TaskDispatchCancelAdapter<'_, '_, '_> {
    type Record = TaskDispatchRecord;

    fn messages(&self) -> RemoteRunCancelMessages {
        RemoteRunCancelMessages {
            run_kind: "task_dispatch",
            canceled: "Canceled remote task dispatch.",
        }
    }

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
        &record.run
    }

    async fn load_latest_record(&self) -> Result<Option<Self::Record>, TrackError> {
        self.service
            .latest_dispatches_for_tasks(std::slice::from_ref(self.task_id))
            .await
            .map(|records| records.into_iter().next())
    }

    fn not_found_error(&self) -> TrackError {
        dispatch_not_found(self.task_id, "does not have a remote dispatch to cancel.")
    }

    fn inactive_error(&self) -> TrackError {
        TrackError::new(
            ErrorCode::DispatchNotFound,
            format!(
                "Task {} does not have an active remote dispatch to cancel.",
                self.task_id
            ),
        )
    }

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .cancel_remote_dispatch_if_possible(record)
            .await
    }

    fn into_canceled_from_ui(&self, record: Self::Record) -> Self::Record {
        record.into_canceled_from_ui()
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .dispatch_repository()
            .save_dispatch(record)
            .await
    }
}
