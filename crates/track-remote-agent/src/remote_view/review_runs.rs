use std::collections::BTreeMap;

use track_types::errors::TrackError;
use track_types::ids::{ProjectId, ReviewId};
use track_types::remote_layout::{DispatchRunDirectory, RemoteCheckoutPath, WorkspaceKey};
use track_types::types::{ReviewRecord, ReviewRunRecord};

use crate::constants::{REMOTE_PROMPT_FILE_NAME, REMOTE_SCHEMA_FILE_NAME};
use crate::remote_actions::{
    CancelRemoteDispatchAction, CleanupReviewArtifactsAction, CreateReviewWorktreeAction,
    LaunchRemoteDispatchAction, UploadRemoteFileAction,
};
use crate::utils::{unique_review_run_directories, unique_review_worktree_paths};

use super::dispatches;
use super::runs;
use super::types::{RemoteArtifactCleanupSummary, RemoteRunSnapshotView, ReviewRunView};
use super::worktrees;
use super::{RemoteWorkspace, RemoteWorktreeEntry};

pub struct ReviewRunRemoteRepository<'a> {
    workspace: &'a RemoteWorkspace,
}

impl<'a> ReviewRunRemoteRepository<'a> {
    pub(super) fn new(workspace: &'a RemoteWorkspace) -> Self {
        Self { workspace }
    }

    pub async fn load_run_views(
        &self,
        review_id: &ReviewId,
    ) -> Result<Vec<ReviewRunView>, TrackError> {
        runs::load_review_run_views(
            &self.workspace.database,
            &self.workspace.remote_agent,
            &self.workspace.ssh_client,
            review_id,
        )
        .await
    }

    pub async fn load_run_views_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<ReviewRunView>, TrackError> {
        runs::load_review_run_views_for_project(
            &self.workspace.database,
            &self.workspace.remote_agent,
            &self.workspace.ssh_client,
            project_id,
        )
        .await
    }

    pub async fn load_snapshots_for_records(
        &self,
        records: &[ReviewRunRecord],
    ) -> Result<Vec<RemoteRunSnapshotView>, TrackError> {
        let run_directories = records
            .iter()
            .map(|record| runs::derive_review_run_directory(record, &self.workspace.remote_agent))
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
        records: &[ReviewRunRecord],
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
            dispatches::list_review_runs_for_project(&self.workspace.database, project_id).await?;
        Ok(runs::list_review_run_directories(
            &self.workspace.remote_agent,
            &records,
        ))
    }

    pub async fn list_worktrees(
        &self,
        workspace_key: &WorkspaceKey,
    ) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
        worktrees::list_review_worktrees(
            &self.workspace.ssh_client,
            &self.workspace.remote_agent,
            workspace_key,
        )
        .await
    }

    pub async fn prepare_worktree(
        &self,
        dispatch_record: &ReviewRunRecord,
        checkout_path: &RemoteCheckoutPath,
        pull_request_number: u64,
        target_head_oid: Option<&str>,
    ) -> Result<(), TrackError> {
        let branch_name = dispatch_record
            .branch_name
            .as_ref()
            .expect("review run records should include branch names before remote launch");
        let worktree_path = dispatch_record
            .worktree_path
            .as_ref()
            .expect("review run records should include worktree paths before remote launch");

        CreateReviewWorktreeAction::new(
            &self.workspace.ssh_client,
            checkout_path,
            pull_request_number,
            branch_name,
            worktree_path,
            target_head_oid,
        )
        .execute()
        .await
    }

    pub async fn upload_run_files(
        &self,
        dispatch_record: &ReviewRunRecord,
        prompt: &str,
        schema: &str,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let run_directory =
            runs::derive_review_run_directory(dispatch_record, &self.workspace.remote_agent);
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
        dispatch_record: &ReviewRunRecord,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let worktree_path = dispatch_record
            .worktree_path
            .as_ref()
            .expect("review run records should include worktree paths before remote launch");
        let run_directory =
            runs::derive_review_run_directory(dispatch_record, &self.workspace.remote_agent);
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
        dispatch_record: &ReviewRunRecord,
    ) -> Result<DispatchRunDirectory, TrackError> {
        let run_directory =
            runs::derive_review_run_directory(dispatch_record, &self.workspace.remote_agent);
        CancelRemoteDispatchAction::new(&self.workspace.ssh_client, &run_directory)
            .execute()
            .await?;

        Ok(run_directory)
    }

    pub async fn cleanup(
        &self,
        review: &ReviewRecord,
        dispatch_history: &[ReviewRunRecord],
    ) -> Result<RemoteArtifactCleanupSummary, TrackError> {
        let branch_names = dispatch_history
            .iter()
            .filter_map(|record| record.branch_name.clone())
            .collect::<Vec<_>>();
        let worktree_paths = unique_review_worktree_paths(dispatch_history);
        let run_directories =
            unique_review_run_directories(dispatch_history, &self.workspace.remote_agent);
        let checkout_path = self
            .workspace
            .projects()
            .resolve_checkout_path_for_workspace(&review.workspace_key);
        let counts = CleanupReviewArtifactsAction::new(
            &self.workspace.ssh_client,
            &checkout_path,
            &branch_names,
            &worktree_paths,
            &run_directories,
        )
        .execute()
        .await?;

        Ok(RemoteArtifactCleanupSummary {
            worktrees_removed: counts.worktrees_removed,
            run_directories_removed: counts.run_directories_removed,
        })
    }
}
