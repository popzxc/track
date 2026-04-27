use track_projects::project_metadata::ProjectMetadata;
use track_types::errors::TrackError;
use track_types::remote_layout::{DispatchBranch, DispatchWorktreePath, RemoteCheckoutPath};
use track_types::types::{RemoteRunState, Task, TaskDispatchRecord};

use super::super::lifecycle::launch::{
    launch_prepared_remote_run, RemoteRunLaunchAdapter, RemoteRunLaunchMessages,
};
use crate::prompts::RemoteDispatchPrompt;
use crate::schemas::RemoteDispatchSchema;

use super::RemoteDispatchService;

pub(super) async fn launch_prepared_dispatch(
    service: &RemoteDispatchService<'_>,
    dispatch_record: TaskDispatchRecord,
) -> Result<TaskDispatchRecord, TrackError> {
    launch_prepared_remote_run(&TaskDispatchLaunchAdapter { service }, dispatch_record).await
}

struct TaskDispatchLaunchContext {
    task: Task,
    project_metadata: ProjectMetadata,
    branch_name: DispatchBranch,
    worktree_path: DispatchWorktreePath,
}

struct TaskDispatchLaunchAdapter<'service, 'database> {
    service: &'service RemoteDispatchService<'database>,
}

#[async_trait::async_trait]
impl RemoteRunLaunchAdapter for TaskDispatchLaunchAdapter<'_, '_> {
    type Record = TaskDispatchRecord;
    type Context = TaskDispatchLaunchContext;

    fn messages(&self) -> RemoteRunLaunchMessages {
        RemoteRunLaunchMessages {
            run_kind: "task_dispatch",
            skipped_inactive: "Skipped launch because dispatch is no longer active.",
            check_prerequisites_summary: "Checking remote agent prerequisites.",
            stopped_before_prerequisites:
                "Launch stopped before prerequisites because dispatch is no longer active.",
            ensure_checkout_summary: "Ensuring the remote checkout is up to date.",
            stopped_before_checkout:
                "Launch stopped while refreshing checkout because dispatch is no longer active.",
            prepare_worktree_summary: "Preparing the task worktree.",
            stopped_before_worktree:
                "Launch stopped while preparing worktree because dispatch is no longer active.",
            upload_files_summary: "Uploading the agent prompt and schema.",
            stopped_before_upload:
                "Launch stopped while uploading run files because dispatch is no longer active.",
            canceled_during_preparation:
                "Launch stopped because dispatch was canceled during preparation.",
            launch_remote_summary: "Launching the remote agent.",
            stopped_before_launch:
                "Launch stopped before remote agent start because dispatch is no longer active.",
            changed_before_promotion:
                "Dispatch changed state before run promotion; returning persisted state.",
            marked_running: "Marked dispatch as running.",
            launch_failed: "Remote task launch failed.",
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
            .dispatch_repository()
            .get_dispatch(&record.task_id, &record.run.dispatch_id)
            .await
    }

    async fn save_record(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .dispatch_repository()
            .save_dispatch(record)
            .await
    }

    async fn save_preparing_phase(
        &self,
        record: &mut Self::Record,
        summary: &str,
    ) -> Result<bool, TrackError> {
        self.service.save_preparing_phase(record, summary).await
    }

    async fn cancel_remote_if_possible(&self, record: &Self::Record) -> Result<(), TrackError> {
        self.service
            .cancel_remote_dispatch_if_possible(record)
            .await
    }

    fn mark_running(&self, record: Self::Record) -> Self::Record {
        record.into_running()
    }

    fn mark_failed(&self, record: Self::Record, error_message: String) -> Self::Record {
        record.into_failed(error_message)
    }

    async fn load_context(&self, record: &Self::Record) -> Result<Self::Context, TrackError> {
        let branch_name = record
            .run
            .branch_name
            .clone()
            .expect("queued dispatches should always store a branch name");
        let worktree_path = record
            .run
            .worktree_path
            .clone()
            .expect("queued dispatches should always store a worktree path");
        let (task, project_metadata) = self
            .service
            .load_dispatch_prerequisites(&record.task_id)
            .await?;
        let remote_agent = self.service.workspace.remote_agent();
        tracing::info!(
            base_branch = %project_metadata.base_branch,
            workspace_root = %remote_agent.workspace_root,
            "Loaded task dispatch prerequisites"
        );

        Ok(TaskDispatchLaunchContext {
            task,
            project_metadata,
            branch_name,
            worktree_path,
        })
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
            .ensure_task_checkout(&context.task.project, &context.project_metadata)
            .await?;
        tracing::info!(checkout_path = ?checkout_path, "Remote checkout is ready");

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
            .task_runs()
            .prepare_worktree(
                record,
                checkout_path,
                &context.project_metadata.base_branch,
                record.run.follow_up_request.is_some(),
            )
            .await?;
        tracing::info!("Prepared remote task worktree");

        Ok(())
    }

    async fn upload_run_files(
        &self,
        record: &Self::Record,
        context: &Self::Context,
    ) -> Result<(), TrackError> {
        let prompt = RemoteDispatchPrompt::new(
            &context.task.project,
            &context.project_metadata,
            &context.branch_name,
            &context.worktree_path,
            &context.task.description,
            record.pull_request_url.as_ref(),
            record.run.follow_up_request.as_deref(),
        )
        .render();
        let schema = RemoteDispatchSchema.render();
        self.service
            .workspace
            .task_runs()
            .upload_run_files(record, &prompt, &schema)
            .await?;
        tracing::info!("Uploaded remote task prompt and schema");

        Ok(())
    }

    async fn launch_remote(
        &self,
        record: &Self::Record,
        _context: &Self::Context,
    ) -> Result<(), TrackError> {
        self.service.workspace.task_runs().launch(record).await?;
        tracing::info!("Started remote task agent process");

        Ok(())
    }
}
