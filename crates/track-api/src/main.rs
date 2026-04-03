use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use track_api::{
    build_app, spawn_remote_review_follow_up_reconciler, AppState, MigrationService,
    RemoteAgentConfigService, SERVER_VERSION_TEXT,
};
use track_dal::dispatch_repository::DispatchRepository;
use track_dal::project_repository::ProjectRepository;
use track_dal::review_dispatch_repository::ReviewDispatchRepository;
use track_dal::review_repository::ReviewRepository;
use track_dal::task_repository::FileTaskRepository;

fn static_root() -> PathBuf {
    if let Ok(path) = env::var("TRACK_STATIC_ROOT") {
        return PathBuf::from(path);
    }

    PathBuf::from("frontend/dist")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_file(true)
        .with_line_number(true)
        .init();

    let config_service = Arc::new(RemoteAgentConfigService::new(None)?);
    let dispatch_repository = Arc::new(DispatchRepository::new(None)?);
    let project_repository = Arc::new(ProjectRepository::new(None)?);
    let review_dispatch_repository = Arc::new(ReviewDispatchRepository::new(None)?);
    let review_repository = Arc::new(ReviewRepository::new(None)?);
    let task_repository = Arc::new(FileTaskRepository::new(None)?);
    let migration_service = Arc::new(MigrationService::new(
        (*config_service).clone(),
        (*project_repository).clone(),
        (*task_repository).clone(),
        (*dispatch_repository).clone(),
        (*review_repository).clone(),
        (*review_dispatch_repository).clone(),
    )?);
    // Docker publishes the backend behind a localhost-only port mapping by
    // default, so the binary still binds all interfaces inside the container.
    // The macOS host-mode smoke runs the binary directly, though, so it needs
    // a narrow bind-host escape hatch to keep that path localhost-only too.
    let bind_host = env::var("TRACK_BIND_HOST").unwrap_or_else(|_| "0.0.0.0".to_owned());
    let port = env::var("PORT").unwrap_or_else(|_| "3210".to_owned());
    let address = format!("{bind_host}:{port}");
    let listener = TcpListener::bind(&address).await?;

    let state = AppState {
        config_service,
        dispatch_repository,
        migration_service,
        project_repository,
        review_dispatch_repository,
        review_repository,
        task_repository,
        task_change_version: Arc::new(AtomicU64::new(0)),
    };
    spawn_remote_review_follow_up_reconciler(state.clone());
    let app = build_app(state, static_root());

    tracing::info!(
        "track API {} listening on http://{}",
        SERVER_VERSION_TEXT,
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;

    Ok(())
}
