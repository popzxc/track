use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use track_core::config::ConfigService;
use track_core::task_repository::FileTaskRepository;

mod app;

use app::{build_app, AppState};

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
        .init();

    let port = env::var("PORT").unwrap_or_else(|_| "3210".to_owned());
    let address = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&address).await?;

    let app = build_app(
        AppState {
            config_service: Arc::new(ConfigService::new(None)?),
            task_repository: Arc::new(FileTaskRepository::new(None)?),
        },
        static_root(),
    );

    tracing::info!("track API listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
