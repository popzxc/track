use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use track_dal::database::DatabaseContext;

use track_remote_agent::RemoteAgentServices;
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::{ActiveRemoteRun, RemoteRunOwner};

use crate::backend_config::RemoteAgentConfigService;

#[derive(Clone)]
pub struct AppState {
    pub config_service: Arc<RemoteAgentConfigService>,
    pub database: DatabaseContext,
    pub remote_agent_config_gate: Arc<RwLock<()>>,
    pub task_change_version: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(config_service: Arc<RemoteAgentConfigService>, database: DatabaseContext) -> Self {
        Self {
            config_service,
            database,
            remote_agent_config_gate: Arc::new(RwLock::new(())),
            task_change_version: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn remote_agent_services(&self) -> RemoteAgentServices<'_> {
        RemoteAgentServices::new(&self.config_service, &self.database)
    }

    pub(crate) async fn remote_agent_operation_guard(&self) -> RwLockReadGuard<'_, ()> {
        self.remote_agent_config_gate.read().await
    }

    pub(crate) async fn remote_agent_config_mutation_guard(&self) -> RwLockWriteGuard<'_, ()> {
        self.remote_agent_config_gate.write().await
    }

    pub(crate) async fn ensure_remote_agent_config_can_change(&self) -> Result<(), TrackError> {
        let active_runs = self
            .database
            .remote_run_repository()
            .active_remote_runs()
            .await?;
        if active_runs.is_empty() {
            return Ok(());
        }

        Err(TrackError::new(
            ErrorCode::RemoteAgentConfigBusy,
            format!(
                "Remote agent settings cannot be changed while remote runs are active. Stop active runs first: {}.",
                describe_active_remote_runs(&active_runs)
            ),
        ))
    }
}

fn describe_active_remote_runs(active_runs: &[ActiveRemoteRun]) -> String {
    const MAX_DESCRIBED_RUNS: usize = 5;

    let mut descriptions = active_runs
        .iter()
        .take(MAX_DESCRIBED_RUNS)
        .map(describe_active_remote_run)
        .collect::<Vec<_>>();
    if active_runs.len() > MAX_DESCRIBED_RUNS {
        descriptions.push(format!(
            "{} more active run(s)",
            active_runs.len() - MAX_DESCRIBED_RUNS
        ));
    }

    descriptions.join(", ")
}

fn describe_active_remote_run(active_run: &ActiveRemoteRun) -> String {
    match &active_run.owner {
        RemoteRunOwner::Task(task_id) => format!(
            "task {task_id} dispatch {} ({})",
            active_run.dispatch_id,
            active_run.status.as_str()
        ),
        RemoteRunOwner::Review(review_id) => format!(
            "review {review_id} run {} ({})",
            active_run.dispatch_id,
            active_run.status.as_str()
        ),
    }
}
