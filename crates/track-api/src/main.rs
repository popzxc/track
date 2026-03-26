use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use track_api::{build_app, spawn_remote_review_follow_up_reconciler, AppState};
use track_core::config::ConfigService;
use track_core::dispatch_repository::DispatchRepository;
use track_core::project_repository::ProjectRepository;
use track_core::task_repository::FileTaskRepository;

fn configured_port(config_service: &ConfigService) -> String {
    match config_service.load_runtime_config() {
        Ok(config) => config.api.port.to_string(),
        Err(_) => "3210".to_owned(),
    }
}

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

    let config_service = Arc::new(ConfigService::new(None)?);
    let port = env::var("PORT").unwrap_or_else(|_| configured_port(&config_service));
    let address = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&address).await?;

    let state = AppState {
        config_service,
        dispatch_repository: Arc::new(DispatchRepository::new(None)?),
        project_repository: Arc::new(ProjectRepository::new(None)?),
        task_repository: Arc::new(FileTaskRepository::new(None)?),
        task_change_version: Arc::new(AtomicU64::new(0)),
    };
    spawn_remote_review_follow_up_reconciler(state.clone());
    let app = build_app(state, static_root());

    tracing::info!("track API listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
