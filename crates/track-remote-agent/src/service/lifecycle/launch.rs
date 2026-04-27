use track_types::errors::TrackError;
use track_types::remote_layout::RemoteCheckoutPath;
use track_types::types::RemoteRunState;

// =============================================================================
// Remote Run Launch Orchestration
// =============================================================================
//
// Launching a task dispatch and launching a PR review run follow the same
// lifecycle shell: refuse stale local records, persist each preparation phase,
// give cancellation a chance to win before the expensive launch, and either
// promote the run to running or persist a terminal failure. The adapter owns
// the domain-specific steps so this orchestration stays about lifecycle, not
// about task prompts or review metadata.
#[async_trait::async_trait]
pub(in crate::service) trait RemoteRunLaunchAdapter: Sync {
    type Record: Clone + Send + Sync;
    type Context: Send + Sync;

    fn messages(&self) -> RemoteRunLaunchMessages;

    fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState;

    async fn load_saved_record(
        &self,
        record: &Self::Record,
    ) -> Result<Option<Self::Record>, TrackError>;

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError>;

    async fn save_preparing_phase(
        &self,
        record: &mut Self::Record,
        summary: &str,
    ) -> Result<bool, TrackError>;

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError>;

    fn into_running(&self, record: Self::Record) -> Self::Record;

    fn into_failed(&self, record: Self::Record, error_message: String) -> Self::Record;

    async fn load_context(&self, record: &Self::Record) -> Result<Self::Context, TrackError>;

    async fn ensure_checkout(
        &self,
        record: &Self::Record,
        context: &Self::Context,
    ) -> Result<RemoteCheckoutPath, TrackError>;

    async fn prepare_worktree(
        &self,
        record: &Self::Record,
        context: &Self::Context,
        checkout_path: &RemoteCheckoutPath,
    ) -> Result<(), TrackError>;

    async fn upload_run_files(
        &self,
        record: &Self::Record,
        context: &Self::Context,
    ) -> Result<(), TrackError>;

    async fn launch_remote(
        &self,
        record: &Self::Record,
        context: &Self::Context,
    ) -> Result<(), TrackError>;
}

#[derive(Debug, Clone, Copy)]
pub(in crate::service) struct RemoteRunLaunchMessages {
    pub(in crate::service) run_kind: &'static str,
    pub(in crate::service) skipped_inactive: &'static str,
    pub(in crate::service) check_prerequisites_summary: &'static str,
    pub(in crate::service) stopped_before_prerequisites: &'static str,
    pub(in crate::service) ensure_checkout_summary: &'static str,
    pub(in crate::service) stopped_before_checkout: &'static str,
    pub(in crate::service) prepare_worktree_summary: &'static str,
    pub(in crate::service) stopped_before_worktree: &'static str,
    pub(in crate::service) upload_files_summary: &'static str,
    pub(in crate::service) stopped_before_upload: &'static str,
    pub(in crate::service) canceled_during_preparation: &'static str,
    pub(in crate::service) launch_remote_summary: &'static str,
    pub(in crate::service) stopped_before_launch: &'static str,
    pub(in crate::service) changed_before_promotion: &'static str,
    pub(in crate::service) marked_running: &'static str,
    pub(in crate::service) launch_failed: &'static str,
}

pub(in crate::service) async fn launch_prepared_remote_run<Adapter>(
    adapter: &Adapter,
    mut record: Adapter::Record,
) -> Result<Adapter::Record, TrackError>
where
    Adapter: RemoteRunLaunchAdapter,
{
    let messages = adapter.messages();
    if let Some(existing_record) = adapter.load_saved_record(&record).await? {
        if !adapter.run(&existing_record).status.is_active() {
            tracing::info!(
                run_kind = messages.run_kind,
                status = ?adapter.run(&existing_record).status,
                reason = messages.skipped_inactive,
                "Skipped remote run launch because local state is no longer active"
            );
            return Ok(existing_record);
        }
    }

    let launch_result = async {
        if !adapter
            .save_preparing_phase(&mut record, messages.check_prerequisites_summary)
            .await?
        {
            tracing::info!("{}", messages.stopped_before_prerequisites);
            return Ok::<(), TrackError>(());
        }
        let context = adapter.load_context(&record).await?;

        if !adapter
            .save_preparing_phase(&mut record, messages.ensure_checkout_summary)
            .await?
        {
            tracing::info!("{}", messages.stopped_before_checkout);
            return Ok::<(), TrackError>(());
        }
        let checkout_path = adapter.ensure_checkout(&record, &context).await?;

        if !adapter
            .save_preparing_phase(&mut record, messages.prepare_worktree_summary)
            .await?
        {
            tracing::info!("{}", messages.stopped_before_worktree);
            return Ok::<(), TrackError>(());
        }
        adapter
            .prepare_worktree(&record, &context, &checkout_path)
            .await?;

        if !adapter
            .save_preparing_phase(&mut record, messages.upload_files_summary)
            .await?
        {
            tracing::info!("{}", messages.stopped_before_upload);
            return Ok::<(), TrackError>(());
        }
        adapter.upload_run_files(&record, &context).await?;

        if !record_is_still_active(adapter, &record).await? {
            tracing::info!("{}", messages.canceled_during_preparation);
            return Ok::<(), TrackError>(());
        }

        if !adapter
            .save_preparing_phase(&mut record, messages.launch_remote_summary)
            .await?
        {
            tracing::info!("{}", messages.stopped_before_launch);
            return Ok::<(), TrackError>(());
        }
        adapter.launch_remote(&record, &context).await?;

        Ok(())
    }
    .await;

    match launch_result {
        Ok(()) => {
            match adapter.load_saved_record(&record).await? {
                Some(existing_record) if !adapter.run(&existing_record).status.is_active() => {
                    let _ = adapter.cancel_remote_if_possible(&existing_record).await;
                    tracing::info!(
                        run_kind = messages.run_kind,
                        status = ?adapter.run(&existing_record).status,
                        reason = messages.changed_before_promotion,
                        "Remote run changed state before promotion; returning persisted state"
                    );
                    return Ok(existing_record);
                }
                Some(_) => {}
                None => {
                    tracing::info!(
                        run_kind = messages.run_kind,
                        dispatch_id = %adapter.run(&record).dispatch_id,
                        reason = messages.changed_before_promotion,
                        "Remote run disappeared before promotion; skipping local resurrection"
                    );
                    return Ok(record);
                }
            }

            let record = adapter.into_running(record);
            adapter.save_record(&record).await?;
            tracing::info!(
                run_kind = messages.run_kind,
                dispatch_id = %adapter.run(&record).dispatch_id,
                detail = messages.marked_running,
                "Marked remote run as running"
            );
            Ok(record)
        }
        Err(error) => {
            tracing::error!(
                run_kind = messages.run_kind,
                error = %error,
                detail = messages.launch_failed,
                "Remote run launch failed"
            );
            let record = adapter.into_failed(record, error.to_string());
            adapter.save_record(&record).await?;
            Err(error)
        }
    }
}

async fn record_is_still_active<Adapter>(
    adapter: &Adapter,
    record: &Adapter::Record,
) -> Result<bool, TrackError>
where
    Adapter: RemoteRunLaunchAdapter,
{
    Ok(adapter
        .load_saved_record(record)
        .await?
        .map(|record| adapter.run(&record).status.is_active())
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use track_types::errors::{ErrorCode, TrackError};
    use track_types::ids::DispatchId;
    use track_types::remote_layout::RemoteCheckoutPath;
    use track_types::time_utils::now_utc;
    use track_types::types::{DispatchStatus, RemoteAgentPreferredTool};

    use super::*;

    #[derive(Debug, Clone)]
    struct TestRecord {
        run: RemoteRunState,
    }

    #[derive(Debug, Default)]
    struct TestLaunchState {
        saved_record: Option<TestRecord>,
        saved_statuses: Vec<DispatchStatus>,
        upload_deletes_record: bool,
        remote_launches: usize,
    }

    #[derive(Clone)]
    struct TestLaunchAdapter {
        state: Arc<Mutex<TestLaunchState>>,
    }

    impl TestLaunchAdapter {
        fn with_saved_record(record: TestRecord) -> Self {
            Self {
                state: Arc::new(Mutex::new(TestLaunchState {
                    saved_record: Some(record),
                    upload_deletes_record: true,
                    ..TestLaunchState::default()
                })),
            }
        }

        fn state(&self) -> std::sync::MutexGuard<'_, TestLaunchState> {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }

    #[async_trait::async_trait]
    impl RemoteRunLaunchAdapter for TestLaunchAdapter {
        type Record = TestRecord;
        type Context = ();

        fn messages(&self) -> RemoteRunLaunchMessages {
            RemoteRunLaunchMessages {
                run_kind: "test_run",
                skipped_inactive: "Skipped inactive test run.",
                check_prerequisites_summary: "Checking test prerequisites.",
                stopped_before_prerequisites: "Stopped before prerequisites.",
                ensure_checkout_summary: "Ensuring test checkout.",
                stopped_before_checkout: "Stopped before checkout.",
                prepare_worktree_summary: "Preparing test worktree.",
                stopped_before_worktree: "Stopped before worktree.",
                upload_files_summary: "Uploading test files.",
                stopped_before_upload: "Stopped before upload.",
                canceled_during_preparation: "Stopped during preparation.",
                launch_remote_summary: "Launching test run.",
                stopped_before_launch: "Stopped before launch.",
                changed_before_promotion: "Changed before promotion.",
                marked_running: "Marked test run running.",
                launch_failed: "Test launch failed.",
            }
        }

        fn run<'record>(&self, record: &'record Self::Record) -> &'record RemoteRunState {
            &record.run
        }

        async fn load_saved_record(
            &self,
            _record: &Self::Record,
        ) -> Result<Option<Self::Record>, TrackError> {
            Ok(self.state().saved_record.clone())
        }

        async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
            let mut state = self.state();
            state.saved_statuses.push(record.run.status);
            state.saved_record = Some(record.clone());
            Ok(())
        }

        async fn save_preparing_phase(
            &self,
            record: &mut Self::Record,
            summary: &str,
        ) -> Result<bool, TrackError> {
            let Some(saved_record) = self.state().saved_record.clone() else {
                return Ok(false);
            };

            if !saved_record.run.status.is_active() {
                *record = saved_record;
                return Ok(false);
            }

            record.run = record.run.clone().into_preparing(summary);
            self.save_record(record).await?;
            Ok(true)
        }

        async fn cancel_remote_if_possible(
            &self,
            _record: &Self::Record,
        ) -> Result<(), TrackError> {
            Ok(())
        }

        fn into_running(&self, mut record: Self::Record) -> Self::Record {
            record.run = record.run.into_running("The test run is running.");
            record
        }

        fn into_failed(&self, mut record: Self::Record, error_message: String) -> Self::Record {
            record.run = record.run.into_failed(error_message);
            record
        }

        async fn load_context(&self, _record: &Self::Record) -> Result<Self::Context, TrackError> {
            Ok(())
        }

        async fn ensure_checkout(
            &self,
            _record: &Self::Record,
            _context: &Self::Context,
        ) -> Result<RemoteCheckoutPath, TrackError> {
            Ok(RemoteCheckoutPath::from_registry_unchecked(
                "~/workspace/project-a/project-a",
            ))
        }

        async fn prepare_worktree(
            &self,
            _record: &Self::Record,
            _context: &Self::Context,
            _checkout_path: &RemoteCheckoutPath,
        ) -> Result<(), TrackError> {
            Ok(())
        }

        async fn upload_run_files(
            &self,
            _record: &Self::Record,
            _context: &Self::Context,
        ) -> Result<(), TrackError> {
            let mut state = self.state();
            if state.upload_deletes_record {
                state.saved_record = None;
            }
            Ok(())
        }

        async fn launch_remote(
            &self,
            _record: &Self::Record,
            _context: &Self::Context,
        ) -> Result<(), TrackError> {
            self.state().remote_launches += 1;
            Err(TrackError::new(
                ErrorCode::InternalError,
                "launch_remote should not be called after local deletion",
            ))
        }
    }

    fn active_test_record() -> TestRecord {
        let timestamp = now_utc();
        TestRecord {
            run: RemoteRunState {
                dispatch_id: DispatchId::new("dispatch-1").expect("dispatch id should parse"),
                preferred_tool: RemoteAgentPreferredTool::Codex,
                status: DispatchStatus::Preparing,
                created_at: timestamp,
                updated_at: timestamp,
                finished_at: None,
                remote_host: "198.51.100.10".to_owned(),
                branch_name: None,
                worktree_path: None,
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: None,
            },
        }
    }

    #[tokio::test]
    async fn launch_does_not_resurrect_a_run_deleted_during_preparation() {
        let record = active_test_record();
        let adapter = TestLaunchAdapter::with_saved_record(record.clone());

        let result = launch_prepared_remote_run(&adapter, record)
            .await
            .expect("deleted local run should stop launch without error");

        let state = adapter.state();
        assert_eq!(result.run.status, DispatchStatus::Preparing);
        assert_eq!(state.remote_launches, 0);
        assert!(
            !state
                .saved_statuses
                .iter()
                .any(|status| *status == DispatchStatus::Running),
            "deleted local runs must not be saved as running again"
        );
        assert!(
            state.saved_record.is_none(),
            "deleted local run should stay deleted after launch exits"
        );
    }
}
