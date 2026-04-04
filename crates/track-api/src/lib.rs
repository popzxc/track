mod app;
mod backend_config;
mod build_info;
mod migration_service;

pub(crate) mod api_error;
pub(crate) mod app_state;
pub(crate) mod routes;

pub use app::{build_app, spawn_remote_review_follow_up_reconciler};
pub use app_state::AppState;
pub use backend_config::{BackendConfigRepository, RemoteAgentConfigService};
pub use build_info::{server_build_info, SERVER_VERSION_TEXT};
pub use migration_service::MigrationService;

#[cfg(test)]
mod test_support {
    pub use track_types::test_support::*;
}

#[cfg(test)]
mod tests;
