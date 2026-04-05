use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use track_dal::database::DatabaseContext;

use track_remote_agent::RemoteAgentServices;

use crate::backend_config::RemoteAgentConfigService;
use crate::migration_service::MigrationService;

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<RemoteAgentConfigService>,
    pub database: DatabaseContext,
    pub migration_service: Arc<MigrationService>,
    pub task_change_version: Arc<AtomicU64>,
}

impl AppState {
    pub(crate) fn remote_agent_services(&self) -> RemoteAgentServices<'_> {
        RemoteAgentServices::new(&self.config_service, &self.database)
    }
}
