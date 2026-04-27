use track_types::errors::TrackError;
use track_types::types::RemoteRunState;

// =============================================================================
// Remote Run Cancellation Orchestration
// =============================================================================
//
// Cancellation is a user-facing lifecycle command shared by task dispatches and
// PR review runs. The shared runner owns the common contract while each domain
// supplies its lookup, remote cancellation, and persistence details.
#[async_trait::async_trait]
pub(in crate::service) trait RemoteRunCancelAdapter: Sync {
    type Record: Send + Sync;

    fn messages(&self) -> RemoteRunCancelMessages;

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState;

    async fn load_latest_record(&self) -> Result<Option<Self::Record>, TrackError>;

    fn not_found_error(&self) -> TrackError;

    fn inactive_error(&self) -> TrackError;

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError>;

    fn into_canceled_from_ui(&self, record: Self::Record) -> Self::Record;

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError>;
}

#[derive(Debug, Clone, Copy)]
pub(in crate::service) struct RemoteRunCancelMessages {
    pub(in crate::service) run_kind: &'static str,
    pub(in crate::service) canceled: &'static str,
}

pub(in crate::service) async fn cancel_latest_remote_run<Adapter>(
    adapter: &Adapter,
) -> Result<Adapter::Record, TrackError>
where
    Adapter: RemoteRunCancelAdapter,
{
    let messages = adapter.messages();
    let latest_record = adapter
        .load_latest_record()
        .await?
        .ok_or_else(|| adapter.not_found_error())?;

    if !adapter.run(&latest_record).status.is_active() {
        return Err(adapter.inactive_error());
    }

    adapter.cancel_remote_if_possible(&latest_record).await?;

    let canceled_record = adapter.into_canceled_from_ui(latest_record);
    adapter.save_record(&canceled_record).await?;

    tracing::info!(
        run_kind = messages.run_kind,
        dispatch_id = %adapter.run(&canceled_record).dispatch_id,
        remote_host = %adapter.run(&canceled_record).remote_host,
        detail = messages.canceled,
        "Canceled remote run"
    );

    Ok(canceled_record)
}
