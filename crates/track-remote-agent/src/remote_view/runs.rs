use std::collections::BTreeMap;

use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_types::errors::TrackError;
use track_types::ids::{ProjectId, ReviewId, TaskId};
use track_types::remote_layout::DispatchRunDirectory;
use track_types::types::{ReviewRunRecord, TaskDispatchRecord};

use crate::remote_actions::ReadDispatchSnapshotsAction;
use crate::ssh::SshClient;

use super::dispatches;
use super::types::{RemoteRunSnapshotView, ReviewRunView, TaskDispatchView};

// =============================================================================
// Remote Run Observation
// =============================================================================
//
// The view layer should expose what the remote machine currently says about a
// run, not apply business rules to local records. These helpers therefore map
// persisted dispatch rows to remote run directories, read the sidecar files in
// batch, and return the observed remote state alongside each local record.
pub(super) async fn load_task_dispatch_views(
    database: &DatabaseContext,
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    task_id: &TaskId,
) -> Result<Vec<TaskDispatchView>, TrackError> {
    let records = database
        .dispatch_repository()
        .dispatches_for_task(task_id)
        .await?;
    map_task_dispatch_views(remote_agent, ssh_client, records).await
}

pub(super) async fn load_task_dispatch_views_for_project(
    database: &DatabaseContext,
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    project_id: &ProjectId,
) -> Result<Vec<TaskDispatchView>, TrackError> {
    let records = dispatches::list_task_dispatches_for_project(database, project_id).await?;
    map_task_dispatch_views(remote_agent, ssh_client, records).await
}

pub(super) async fn load_review_run_views(
    database: &DatabaseContext,
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    review_id: &ReviewId,
) -> Result<Vec<ReviewRunView>, TrackError> {
    let records = database
        .review_dispatch_repository()
        .dispatches_for_review(review_id)
        .await?;
    map_review_run_views(remote_agent, ssh_client, records).await
}

pub(super) async fn load_review_run_views_for_project(
    database: &DatabaseContext,
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    project_id: &ProjectId,
) -> Result<Vec<ReviewRunView>, TrackError> {
    let records = dispatches::list_review_runs_for_project(database, project_id).await?;
    map_review_run_views(remote_agent, ssh_client, records).await
}

pub(super) fn list_task_run_directories(
    remote_agent: &RemoteAgentRuntimeConfig,
    records: &[TaskDispatchRecord],
) -> Vec<DispatchRunDirectory> {
    let mut run_directories = records
        .iter()
        .map(|record| derive_task_run_directory(record, remote_agent))
        .collect::<Vec<_>>();
    run_directories.sort();
    run_directories.dedup();
    run_directories
}

pub(super) fn list_review_run_directories(
    remote_agent: &RemoteAgentRuntimeConfig,
    records: &[ReviewRunRecord],
) -> Vec<DispatchRunDirectory> {
    let mut run_directories = records
        .iter()
        .map(|record| derive_review_run_directory(record, remote_agent))
        .collect::<Vec<_>>();
    run_directories.sort();
    run_directories.dedup();
    run_directories
}

async fn map_task_dispatch_views(
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    records: Vec<TaskDispatchRecord>,
) -> Result<Vec<TaskDispatchView>, TrackError> {
    let run_directories = records
        .iter()
        .map(|record| derive_task_run_directory(record, remote_agent))
        .collect::<Vec<_>>();
    let snapshots = load_snapshots(ssh_client, &run_directories).await?;

    Ok(records
        .into_iter()
        .zip(run_directories)
        .map(|(record, run_directory)| {
            let snapshot = snapshots.get(run_directory.as_str());
            TaskDispatchView {
                record,
                remote: snapshot_view_or_missing(run_directory, snapshot),
            }
        })
        .collect())
}

async fn map_review_run_views(
    remote_agent: &RemoteAgentRuntimeConfig,
    ssh_client: &SshClient,
    records: Vec<ReviewRunRecord>,
) -> Result<Vec<ReviewRunView>, TrackError> {
    let run_directories = records
        .iter()
        .map(|record| derive_review_run_directory(record, remote_agent))
        .collect::<Vec<_>>();
    let snapshots = load_snapshots(ssh_client, &run_directories).await?;

    Ok(records
        .into_iter()
        .zip(run_directories)
        .map(|(record, run_directory)| {
            let snapshot = snapshots.get(run_directory.as_str());
            ReviewRunView {
                record,
                remote: snapshot_view_or_missing(run_directory, snapshot),
            }
        })
        .collect())
}

pub(super) fn derive_task_run_directory(
    record: &TaskDispatchRecord,
    remote_agent: &RemoteAgentRuntimeConfig,
) -> DispatchRunDirectory {
    if let Some(worktree_path) = record.worktree_path.as_ref() {
        return worktree_path.run_directory_for(&record.dispatch_id);
    }

    DispatchRunDirectory::for_task(
        &remote_agent.workspace_root,
        &record.project,
        &record.dispatch_id,
    )
}

pub(super) fn derive_review_run_directory(
    record: &ReviewRunRecord,
    remote_agent: &RemoteAgentRuntimeConfig,
) -> DispatchRunDirectory {
    if let Some(worktree_path) = record.worktree_path.as_ref() {
        return worktree_path.run_directory();
    }

    DispatchRunDirectory::for_review(
        &remote_agent.workspace_root,
        &record.workspace_key,
        &record.dispatch_id,
    )
}

pub(super) async fn load_snapshots(
    ssh_client: &SshClient,
    run_directories: &[DispatchRunDirectory],
) -> Result<BTreeMap<String, RemoteRunSnapshotView>, TrackError> {
    if run_directories.is_empty() {
        return Ok(BTreeMap::new());
    }

    let snapshots = ReadDispatchSnapshotsAction::new(ssh_client, run_directories)
        .execute()
        .await?;
    Ok(snapshots
        .into_iter()
        .map(|snapshot| (snapshot.run_directory.as_str().to_owned(), snapshot))
        .collect())
}

pub(super) fn snapshot_view_or_missing(
    run_directory: DispatchRunDirectory,
    snapshot: Option<&RemoteRunSnapshotView>,
) -> RemoteRunSnapshotView {
    snapshot
        .cloned()
        .unwrap_or_else(|| RemoteRunSnapshotView::missing(run_directory))
}
