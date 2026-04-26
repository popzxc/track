use std::collections::BTreeMap;

use track_types::errors::TrackError;
use track_types::ids::{ProjectId, TaskId};
use track_types::remote_layout::{DispatchRunDirectory, RemoteCheckoutPath};
use track_types::types::TaskDispatchRecord;

use crate::constants::{REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME};
use crate::remote_actions::{
    CancelRemoteDispatchAction, CleanupTaskArtifactsAction, CreateWorktreeAction,
    EnsureFollowUpWorktreeAction, LaunchRemoteDispatchAction, UploadRemoteFileAction,
};
use crate::types::RemoteTaskCleanupMode;

use super::dispatches;
use super::runs;
use super::types::{
    RemoteArtifactCleanupSummary, RemoteRunSnapshotView, RemoteTaskArtifactCleanupMode,
    TaskDispatchView,
};
use super::worktrees;
use super::{RemoteWorkspace, RemoteWorktreeEntry};

pub struct TaskRunRemoteRepository<'a> {
    workspace: &'a RemoteWorkspace,
}

impl<'a> TaskRunRemoteRepository<'a> {
    pub(super) fn new(workspace: &'a RemoteWorkspace) -> Self {
        Self { workspace }
    }

    pub async fn load_dispatch_views(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<TaskDispatchView>, TrackError> {
        runs::load_task_dispatch_views(
            &self.workspace.database,
            &self.workspace.remote_agent,
            &self.workspace.ssh_client,
            task_id,
        )
        .await
    }

    pub async fn load_dispatch_views_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<TaskDispatchView>, TrackError> {
        runs::load_task_dispatch_views_for_project(
            &self.workspace.database,
            &self.workspace.remote_agent,
            &self.workspace.ssh_client,
            project_id,
        )
        .await
    }

    pub async fn load_snapshots_for_records(
        &self,
        records: &[TaskDispatchRecord],
    ) -> Result<Vec<RemoteRunSnapshotView>, TrackError> {
        let run_directories = records
            .iter()
            .map(|record| runs::derive_task_run_directory(record, &self.workspace.remote_agent))
            .collect::<Vec<_>>();
        let snapshots = runs::load_snapshots(&self.workspace.ssh_client, &run_directories).await?;

        Ok(run_directories
            .into_iter()
            .map(|run_directory| {
                let snapshot = snapshots.get(run_directory.as_str());
                runs::snapshot_view_or_missing(run_directory, snapshot)
            })
            .collect())
    }

    pub async fn load_snapshots_for_active_records(
        &self,
        records: &[TaskDispatchRecord],
    ) -> Result<BTreeMap<String, RemoteRunSnapshotView>, TrackError> {
        let active_records = records
            .iter()
            .filter(|record| record.status.is_active())
            .cloned()
            .collect::<Vec<_>>();
        if active_records.is_empty() {
            return Ok(BTreeMap::new());
        }

        let snapshots = self.load_snapshots_for_records(&active_records).await?;
        Ok(active_records
            .into_iter()
            .zip(snapshots)
            .map(|(record, snapshot)| (record.dispatch_id.to_string(), snapshot))
            .collect())
    }

    pub async fn list_run_directories_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<DispatchRunDirectory>, TrackError> {
        let records =
            dispatches::list_task_dispatches_for_project(&self.workspace.database, project_id)
                .await?;
        Ok(runs::list_task_run_directories(
            &self.workspace.remote_agent,
            &records,
        ))
    }

    pub async fn list_worktrees(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
        worktrees::list_task_worktrees(
            &self.workspace.ssh_client,
            &self.workspace.remote_agent,
            project_id,
        )
        .await
    }

    pub async fn prepare_worktree(
        &self,
        dispatch_record: &TaskDispatchRecord,
        checkout_path: &RemoteCheckoutPath,
        base_branch: &str,
        reuse_existing_worktree: bool,
    ) -> Result<(), TrackError> {
        let branch_name = dispatch_record
            .branch_name
            .as_ref()
            .expect("task dispatch records should include branch names before remote launch");
        let worktree_path = dispatch_record
            .worktree_path
            .as_ref()
            .expect("task dispatch records should include worktree paths before remote launch");

        if reuse_existing_worktree {
            EnsureFollowUpWorktreeAction::new(
                &self.workspace.ssh_client,
                checkout_path,
                branch_name,
                worktree_path,
            )
            .execute()
            .await
        } else {
            CreateWorktreeAction::new(
                &self.workspace.ssh_client,
                checkout_path,
                base_branch,
                branch_name,
                worktree_path,
            )
            .execute()
            .await
        }
    }

    pub async fn upload_run_files(
        &self,
        dispatch_record: &TaskDispatchRecord,
        prompt: &str,
        schema: &str,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let run_directory =
            runs::derive_task_run_directory(dispatch_record, &self.workspace.remote_agent);
        UploadRemoteFileAction::new(
            &self.workspace.ssh_client,
            &run_directory.join(REMOTE_PROMPT_FILE_NAME),
            prompt,
        )
        .execute()
        .await?;
        UploadRemoteFileAction::new(
            &self.workspace.ssh_client,
            &run_directory.join(REMOTE_SCHEMA_FILE_NAME),
            schema,
        )
        .execute()
        .await?;

        Ok(run_directory)
    }

    pub async fn launch(
        &self,
        dispatch_record: &TaskDispatchRecord,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let worktree_path = dispatch_record
            .worktree_path
            .as_ref()
            .expect("task dispatch records should include worktree paths before remote launch");
        let run_directory =
            runs::derive_task_run_directory(dispatch_record, &self.workspace.remote_agent);
        LaunchRemoteDispatchAction::new(
            &self.workspace.ssh_client,
            &run_directory,
            worktree_path,
            dispatch_record.preferred_tool,
        )
        .execute()
        .await?;

        Ok(run_directory)
    }

    pub async fn cancel(
        &self,
        dispatch_record: &TaskDispatchRecord,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let run_directory =
            runs::derive_task_run_directory(dispatch_record, &self.workspace.remote_agent);
        CancelRemoteDispatchAction::new(&self.workspace.ssh_client, &run_directory)
            .execute()
            .await?;

        Ok(run_directory)
    }

    pub async fn cleanup(
        &self,
        checkout_path: &RemoteCheckoutPath,
        dispatch_history: &[TaskDispatchRecord],
        cleanup_mode: RemoteTaskArtifactCleanupMode,
    ) -> Result<RemoteArtifactCleanupSummary, TrackError> {
        let counts = CleanupTaskArtifactsAction::new(
            &self.workspace.ssh_client,
            checkout_path,
            &unique_task_worktree_paths(dispatch_history),
            &runs::list_task_run_directories(&self.workspace.remote_agent, dispatch_history),
            match cleanup_mode {
                RemoteTaskArtifactCleanupMode::CloseTask => RemoteTaskCleanupMode::CloseTask,
                RemoteTaskArtifactCleanupMode::DeleteTask => RemoteTaskCleanupMode::DeleteTask,
            },
        )
        .execute()
        .await?;

        Ok(RemoteArtifactCleanupSummary {
            worktrees_removed: counts.worktrees_removed,
            run_directories_removed: counts.run_directories_removed,
        })
    }
}

fn unique_task_worktree_paths(
    dispatch_history: &[TaskDispatchRecord],
) -> Vec<track_types::remote_layout::DispatchWorktreePath> {
    let mut paths = dispatch_history
        .iter()
        .filter_map(|record| record.worktree_path.clone())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}
