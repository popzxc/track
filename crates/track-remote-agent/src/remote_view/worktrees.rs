use track_config::runtime::RemoteAgentRuntimeConfig;
use track_types::errors::TrackError;
use track_types::ids::ProjectId;
use track_types::remote_layout::{DispatchWorktreePath, WorkspaceKey};

use crate::constants::REVIEW_WORKTREE_DIRECTORY_NAME;
use crate::ssh::SshClient;

use super::filesystem;
use super::types::{RemoteWorktreeEntry, RemoteWorktreeKind};

const TASK_WORKTREE_DIRECTORY_NAME: &str = "worktrees";

pub(super) async fn list_task_worktrees(
    ssh_client: &SshClient,
    remote_agent: &RemoteAgentRuntimeConfig,
    project_id: &ProjectId,
) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
    list_worktrees_for_directory(
        ssh_client,
        workspace_root_path(
            &remote_agent.workspace_root,
            &project_id.as_workspace_key(),
            TASK_WORKTREE_DIRECTORY_NAME,
        ),
        RemoteWorktreeKind::Task,
    )
    .await
}

pub(super) async fn list_review_worktrees(
    ssh_client: &SshClient,
    remote_agent: &RemoteAgentRuntimeConfig,
    workspace_key: &WorkspaceKey,
) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
    list_worktrees_for_directory(
        ssh_client,
        workspace_root_path(
            &remote_agent.workspace_root,
            workspace_key,
            REVIEW_WORKTREE_DIRECTORY_NAME,
        ),
        RemoteWorktreeKind::Review,
    )
    .await
}

async fn list_worktrees_for_directory(
    ssh_client: &SshClient,
    directory_path: String,
    kind: RemoteWorktreeKind,
) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
    let mut worktrees = filesystem::list_directories(ssh_client, &directory_path)
        .await?
        .into_iter()
        .map(|path| {
            let path = DispatchWorktreePath::new(&path)?;
            Ok(RemoteWorktreeEntry {
                kind,
                run_directory: path.run_directory(),
                path,
            })
        })
        .collect::<Result<Vec<_>, TrackError>>()?;
    worktrees.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(worktrees)
}

fn workspace_root_path(
    workspace_root: &str,
    workspace_key: &WorkspaceKey,
    directory_name: &str,
) -> String {
    format!(
        "{}/{}/{}",
        workspace_root.trim_end_matches('/'),
        workspace_key.as_str(),
        directory_name
    )
}
