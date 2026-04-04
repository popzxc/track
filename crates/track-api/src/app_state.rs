use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;

use track_remote_agent::RemoteAgentServices;

use crate::backend_config::RemoteAgentConfigService;
use crate::migration_service::MigrationService;

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<RemoteAgentConfigService>,
    pub dispatch_repository: Arc<DispatchRepository>,
    pub migration_service: Arc<MigrationService>,
    pub project_repository: Arc<ProjectRepository>,
    pub review_dispatch_repository: Arc<ReviewDispatchRepository>,
    pub review_repository: Arc<ReviewRepository>,
    pub task_repository: Arc<FileTaskRepository>,
    pub task_change_version: Arc<AtomicU64>,
}

impl AppState {
    pub(crate) fn remote_agent_services(&self) -> RemoteAgentServices<'_> {
        RemoteAgentServices::new(
            &self.config_service,
            &self.dispatch_repository,
            &self.project_repository,
            &self.task_repository,
            &self.review_repository,
            &self.review_dispatch_repository,
        )
    }
}
