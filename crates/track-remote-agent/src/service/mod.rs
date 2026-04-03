use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;
use track_types::errors::TrackError;

mod cleanup;
mod dispatch;
mod follow_up;
mod refresh;
mod review;
mod start_gate;

pub trait RemoteAgentConfigProvider {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError>;
}

type RemoteAgentConfigService = dyn RemoteAgentConfigProvider;

impl<T: RemoteAgentConfigProvider + ?Sized> RemoteAgentConfigProvider for std::sync::Arc<T> {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        (**self).load_remote_agent_runtime_config()
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct StaticRemoteAgentConfigService {
    remote_agent: Option<RemoteAgentRuntimeConfig>,
}

#[cfg(test)]
impl StaticRemoteAgentConfigService {
    pub(crate) fn new(remote_agent: Option<RemoteAgentRuntimeConfig>) -> Self {
        Self { remote_agent }
    }
}

#[cfg(test)]
impl RemoteAgentConfigProvider for StaticRemoteAgentConfigService {
    fn load_remote_agent_runtime_config(
        &self,
    ) -> Result<Option<RemoteAgentRuntimeConfig>, TrackError> {
        Ok(self.remote_agent.clone())
    }
}

pub struct RemoteDispatchService<'a> {
    pub config_service: &'a RemoteAgentConfigService,
    pub dispatch_repository: &'a DispatchRepository,
    pub project_repository: &'a ProjectRepository,
    pub task_repository: &'a FileTaskRepository,
    pub review_repository: &'a ReviewRepository,
    pub review_dispatch_repository: &'a ReviewDispatchRepository,
}

pub struct RemoteReviewService<'a> {
    pub config_service: &'a RemoteAgentConfigService,
    pub project_repository: &'a ProjectRepository,
    pub review_repository: &'a ReviewRepository,
    pub review_dispatch_repository: &'a ReviewDispatchRepository,
}

impl<'a> RemoteDispatchService<'a> {
    fn review_service(&self) -> RemoteReviewService<'_> {
        RemoteReviewService {
            config_service: self.config_service,
            project_repository: self.project_repository,
            review_repository: self.review_repository,
            review_dispatch_repository: self.review_dispatch_repository,
        }
    }
}

#[cfg(test)]
mod tests;
