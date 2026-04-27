mod sqlx_onboarding;

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use sqlx::migrate::Migrator;
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};
use track_config::paths::{get_backend_database_path, path_to_string, DATABASE_FILE_NAME};
use track_types::errors::{ErrorCode, TrackError};

use crate::dispatch_repository::DispatchRepository;
use crate::project_repository::ProjectRepository;
use crate::review_dispatch_repository::ReviewDispatchRepository;
use crate::review_repository::ReviewRepository;
use crate::settings_repository::SettingsRepository;
use crate::task_repository::FileTaskRepository;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
const SQLITE_POOL_MAX_CONNECTIONS: u32 = 8;

#[derive(Debug, Clone)]
pub struct DatabaseContext {
    pool: SqlitePool,
}

impl DatabaseContext {
    async fn uninitialized(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database_path = resolve_database_path(database_path)?;
        let connect_options = connect_options(&database_path)?;
        let pool = SqlitePoolOptions::new()
            // The frontend bootstraps several independent read endpoints in
            // parallel, while background reconciliation can be active at the
            // same time. With WAL enabled, SQLite can serve those concurrent
            // readers safely, so a single pooled connection is needlessly
            // restrictive and turns unrelated requests into one queue.
            .max_connections(SQLITE_POOL_MAX_CONNECTIONS)
            .connect_with(connect_options)
            .await
            .database_error_with(format!(
                "Could not open the SQLite database at {}",
                path_to_string(&database_path)
            ))?;

        Ok(Self { pool })
    }

    pub async fn initialized(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = Self::uninitialized(database_path).await?;
        database.initialize().await?;
        Ok(database)
    }

    pub async fn connect(&self) -> Result<PoolConnection<Sqlite>, TrackError> {
        self.pool
            .acquire()
            .await
            .database_error_with("Could not acquire a SQLite connection from the backend pool")
    }

    pub async fn begin(&self) -> Result<Transaction<'_, Sqlite>, TrackError> {
        self.pool
            .begin()
            .await
            .database_error_with("Could not begin a SQLite transaction")
    }

    async fn initialize(&self) -> Result<(), TrackError> {
        let mut connection = self.connect().await?;

        // Existing user databases predate SQLx's migration table, so we first
        // baseline that legacy fully-initialized schema before handing control
        // to the embedded migrator. Fresh databases skip the onboarding path
        // and let SQLx create the schema from scratch as usual.
        sqlx_onboarding::baseline_existing_schema_if_needed(&mut connection, &MIGRATOR).await?;

        MIGRATOR
            .run_direct(&mut *connection)
            .await
            .database_error_with("Could not initialize the SQLite schema via SQLx migrations")
    }

    pub fn dispatch_repository(&self) -> DispatchRepository<'_> {
        DispatchRepository::new(self)
    }

    pub fn project_repository(&self) -> ProjectRepository<'_> {
        ProjectRepository::new(self)
    }

    pub fn review_dispatch_repository(&self) -> ReviewDispatchRepository<'_> {
        ReviewDispatchRepository::new(self)
    }

    pub fn review_repository(&self) -> ReviewRepository<'_> {
        ReviewRepository::new(self)
    }

    pub fn settings_repository(&self) -> SettingsRepository<'_> {
        SettingsRepository::new(self)
    }

    pub fn task_repository(&self) -> FileTaskRepository<'_> {
        FileTaskRepository::new(self)
    }
}

pub(crate) fn resolve_database_path(database_path: Option<PathBuf>) -> Result<PathBuf, TrackError> {
    match database_path {
        Some(database_path)
            if database_path.extension().and_then(|value| value.to_str()) == Some("sqlite") =>
        {
            Ok(database_path)
        }
        Some(database_path) => Ok(database_path.join(DATABASE_FILE_NAME)),
        None => get_backend_database_path(),
    }
}

fn connect_options(database_path: &Path) -> Result<SqliteConnectOptions, TrackError> {
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            TrackError::new(
                ErrorCode::InternalError,
                format!(
                    "Could not create the backend state directory at {}: {error}",
                    path_to_string(parent)
                ),
            )
        })?;
    }

    Ok(SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal))
}

pub trait DatabaseResultExt<T> {
    fn database_error(self) -> Result<T, TrackError>;

    fn database_error_with<C>(self, context: C) -> Result<T, TrackError>
    where
        C: Into<String>;
}

impl<T> DatabaseResultExt<T> for Result<T, sqlx::Error> {
    fn database_error(self) -> Result<T, TrackError> {
        self.map_err(track_database_error)
    }

    fn database_error_with<C>(self, context: C) -> Result<T, TrackError>
    where
        C: Into<String>,
    {
        self.map_err(|error| contextual_database_error(context, error))
    }
}

impl<T> DatabaseResultExt<T> for Result<T, sqlx::migrate::MigrateError> {
    fn database_error(self) -> Result<T, TrackError> {
        self.map_err(track_database_error)
    }

    fn database_error_with<C>(self, context: C) -> Result<T, TrackError>
    where
        C: Into<String>,
    {
        self.map_err(|error| contextual_database_error(context, error))
    }
}

fn track_database_error(error: impl Debug) -> TrackError {
    TrackError::new(
        ErrorCode::InternalError,
        format!("Database error: {error:?}"),
    )
}

fn contextual_database_error(context: impl Into<String>, error: impl Debug) -> TrackError {
    TrackError::new(
        ErrorCode::InternalError,
        format!("{}. Database error: {error:?}", context.into()),
    )
}

#[cfg(test)]
mod tests {
    use sqlx::Row;
    use tempfile::TempDir;

    use super::DatabaseContext;

    const EXPECTED_APPLICATION_TABLES: &[&str] = &[
        "projects",
        "project_aliases",
        "tasks",
        "reviews",
        "remote_runs",
        "task_run_details",
        "review_run_details",
        "backend_settings",
    ];

    #[tokio::test]
    async fn initialize_applies_the_embedded_sqlx_migration_to_a_fresh_database() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::uninitialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database context should resolve");

        database
            .initialize()
            .await
            .expect("database schema should initialize");

        let mut connection = database
            .connect()
            .await
            .expect("database connection should open");
        let row = sqlx::query("SELECT COUNT(*) AS count FROM _sqlx_migrations")
            .fetch_one(&mut *connection)
            .await
            .expect("migration count query should succeed");
        let migration_count = row.get::<i64, _>("count");
        assert_eq!(migration_count, 2);

        for table_name in EXPECTED_APPLICATION_TABLES {
            let row = sqlx::query(
                "SELECT 1 AS found FROM sqlite_master WHERE type = 'table' AND name = ?1",
            )
            .bind(table_name)
            .fetch_optional(&mut *connection)
            .await
            .expect("table existence query should succeed");

            assert!(
                row.is_some(),
                "expected table {table_name} to exist after initialization"
            );
        }
    }
}
