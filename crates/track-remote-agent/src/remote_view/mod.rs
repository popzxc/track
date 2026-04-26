mod dispatches;
mod filesystem;
mod maintenance;
mod projects;
mod review_runs;
mod runs;
mod task_runs;
mod types;
mod worktrees;

use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_types::errors::TrackError;
use track_types::ids::{ProjectId, ReviewId, TaskId};
use track_types::remote_layout::{DispatchRunDirectory, RemoteCheckoutPath, WorkspaceKey};
use track_types::types::ReviewRecord;

use crate::ssh::SshClient;

pub use maintenance::RemoteMaintenanceRepository;
pub use projects::ProjectRemoteRepository;
pub use review_runs::ReviewRunRemoteRepository;
pub use task_runs::TaskRunRemoteRepository;
pub use types::{
    RemoteArtifactCleanupSummary, RemoteProjectSnapshot, RemotePullRequestMetadata,
    RemotePullRequestReviewState, RemoteRunObservedStatus, RemoteRunSnapshotView,
    RemoteSubmittedReview, RemoteTaskArtifactCleanupMode, RemoteWorktreeEntry, RemoteWorktreeKind,
    ReviewRunView, TaskDispatchView,
};

// =============================================================================
// Remote Workspace
// =============================================================================
//
// This type owns the runtime config, database context, and SSH client needed
// to interact with the remote workspace. Domain-scoped repository handles hang
// off the root so read and write operations can stay grouped by the remote
// concepts they operate on instead of collapsing back into one service object.
pub struct RemoteWorkspace {
    database: DatabaseContext,
    remote_agent: RemoteAgentRuntimeConfig,
    ssh_client: SshClient,
}

pub type RemoteWorkspaceView = RemoteWorkspace;

impl RemoteWorkspace {
    pub fn new(
        remote_agent: RemoteAgentRuntimeConfig,
        database: DatabaseContext,
    ) -> Result<Self, TrackError> {
        let ssh_client = SshClient::new(&remote_agent)?;

        Ok(Self {
            database,
            remote_agent,
            ssh_client,
        })
    }

    pub fn projects(&self) -> ProjectRemoteRepository<'_> {
        ProjectRemoteRepository::new(self)
    }

    pub fn task_runs(&self) -> TaskRunRemoteRepository<'_> {
        TaskRunRemoteRepository::new(self)
    }

    pub fn review_runs(&self) -> ReviewRunRemoteRepository<'_> {
        ReviewRunRemoteRepository::new(self)
    }

    pub fn maintenance(&self) -> RemoteMaintenanceRepository<'_> {
        RemoteMaintenanceRepository::new(self)
    }

    pub fn resolve_checkout_path_for_project(&self, project_id: &ProjectId) -> RemoteCheckoutPath {
        self.projects()
            .resolve_checkout_path_for_project(project_id)
    }

    pub fn resolve_checkout_path_for_review_workspace(
        &self,
        workspace_key: &WorkspaceKey,
    ) -> RemoteCheckoutPath {
        self.projects()
            .resolve_checkout_path_for_workspace(workspace_key)
    }

    pub async fn load_task_dispatch_views(
        &self,
        task_id: &TaskId,
    ) -> Result<Vec<TaskDispatchView>, TrackError> {
        self.task_runs().load_dispatch_views(task_id).await
    }

    pub async fn load_task_dispatch_views_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<TaskDispatchView>, TrackError> {
        self.task_runs()
            .load_dispatch_views_for_project(project_id)
            .await
    }

    pub async fn load_review_run_views(
        &self,
        review_id: &ReviewId,
    ) -> Result<Vec<ReviewRunView>, TrackError> {
        self.review_runs().load_run_views(review_id).await
    }

    pub async fn load_review_run_views_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<ReviewRunView>, TrackError> {
        self.review_runs()
            .load_run_views_for_project(project_id)
            .await
    }

    pub async fn list_task_run_directories_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<DispatchRunDirectory>, TrackError> {
        self.task_runs()
            .list_run_directories_for_project(project_id)
            .await
    }

    pub async fn list_review_run_directories_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<DispatchRunDirectory>, TrackError> {
        self.review_runs()
            .list_run_directories_for_project(project_id)
            .await
    }

    pub async fn list_task_worktrees(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
        self.task_runs().list_worktrees(project_id).await
    }

    pub async fn list_review_worktrees(
        &self,
        workspace_key: &WorkspaceKey,
    ) -> Result<Vec<RemoteWorktreeEntry>, TrackError> {
        self.review_runs().list_worktrees(workspace_key).await
    }

    pub async fn load_project_snapshot(
        &self,
        project_id: &ProjectId,
    ) -> Result<RemoteProjectSnapshot, TrackError> {
        let project = self
            .database
            .project_repository()
            .get_project_by_name(project_id)
            .await?;
        let task_dispatches = self
            .task_runs()
            .load_dispatch_views_for_project(&project.canonical_name)
            .await?;
        let reviews = self
            .list_reviews_for_project(&project.canonical_name)
            .await?;
        let review_runs = self
            .review_runs()
            .load_run_views_for_project(&project.canonical_name)
            .await?;
        let task_worktrees = self
            .task_runs()
            .list_worktrees(&project.canonical_name)
            .await?;

        let mut workspace_keys = reviews
            .iter()
            .map(|review| review.workspace_key.clone())
            .collect::<Vec<_>>();
        if workspace_keys.is_empty() {
            workspace_keys.push(project.canonical_name.as_workspace_key());
        }
        workspace_keys.sort();
        workspace_keys.dedup();

        let mut review_worktrees = Vec::new();
        for workspace_key in workspace_keys {
            review_worktrees.extend(self.review_runs().list_worktrees(&workspace_key).await?);
        }
        review_worktrees.sort_by(|left, right| left.path.cmp(&right.path));

        Ok(RemoteProjectSnapshot {
            project,
            task_dispatches,
            reviews,
            review_runs,
            task_worktrees,
            review_worktrees,
        })
    }

    async fn list_reviews_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<ReviewRecord>, TrackError> {
        dispatches::list_reviews_for_project(&self.database, project_id).await
    }
}
