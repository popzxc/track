use std::sync::Arc;

use track_config::runtime::RemoteAgentRuntimeConfig;
use track_dal::database::DatabaseContext;
use track_types::errors::TrackError;

use crate::RemoteWorkspace;

use super::dispatch::RemoteDispatchService;
use super::maintenance::RemoteWorkspaceMaintenanceService;
use super::review::RemoteReviewService;
use super::review_follow_up::ReviewFollowUpService;

pub struct RemoteAgentRuntimeServices<'a> {
    database: &'a DatabaseContext,
    workspace: Arc<RemoteWorkspace>,
}

impl<'a> RemoteAgentRuntimeServices<'a> {
    pub fn new(
        remote_agent: RemoteAgentRuntimeConfig,
        database: &'a DatabaseContext,
    ) -> Result<Self, TrackError> {
        Ok(Self {
            database,
            workspace: Arc::new(RemoteWorkspace::new(remote_agent, database.clone())?),
        })
    }

    pub fn dispatch(&self) -> RemoteDispatchService<'a> {
        RemoteDispatchService {
            database: self.database,
            workspace: Arc::clone(&self.workspace),
        }
    }

    pub fn review(&self) -> RemoteReviewService<'a> {
        RemoteReviewService {
            database: self.database,
            workspace: Arc::clone(&self.workspace),
        }
    }

    pub fn maintenance(&self) -> RemoteWorkspaceMaintenanceService<'a> {
        RemoteWorkspaceMaintenanceService::new(self.database, Arc::clone(&self.workspace))
    }

    pub fn review_follow_up(&self) -> ReviewFollowUpService<'a> {
        ReviewFollowUpService::new(self.database, Arc::clone(&self.workspace))
    }
}
