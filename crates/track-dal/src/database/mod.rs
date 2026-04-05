mod sqlx_onboarding;

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{Connection, SqliteConnection};
use track_config::paths::{get_backend_database_path, path_to_string, DATABASE_FILE_NAME};
use track_types::errors::{ErrorCode, TrackError};

type BoxDbFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, TrackError>> + Send + 'a>>;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
pub struct DatabaseContext {
    database_path: PathBuf,
}

impl DatabaseContext {
    pub fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database_path = match database_path {
            Some(database_path)
                if database_path.extension().and_then(|value| value.to_str()) == Some("sqlite") =>
            {
                database_path
            }
            Some(database_path) => database_path.join(DATABASE_FILE_NAME),
            None => get_backend_database_path()?,
        };

        Ok(Self { database_path })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub async fn initialize(&self) -> Result<(), TrackError> {
        self.run(|connection| {
            Box::pin(async move {
                // Existing user databases predate SQLx's migration table, so
                // we first baseline that legacy fully-initialized schema before
                // handing control to the embedded migrator. Fresh databases
                // skip the onboarding path and let SQLx create the schema from
                // scratch as usual.
                sqlx_onboarding::baseline_existing_schema_if_needed(connection, &MIGRATOR).await?;

                MIGRATOR.run_direct(connection).await.map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!(
                            "Could not initialize the SQLite schema via SQLx migrations: {error}"
                        ),
                    )
                })?;

                Ok(())
            })
        })
        .await
    }

    pub async fn run<T>(
        &self,
        operation: impl for<'a> FnOnce(&'a mut SqliteConnection) -> BoxDbFuture<'a, T> + Send + 'static,
    ) -> Result<T, TrackError>
    where
        T: Send + 'static,
    {
        let connect_options = self.connect_options()?;
        let database_path = self.database_path.clone();

        let mut connection = SqliteConnection::connect_with(&connect_options)
            .await
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not open the SQLite database at {}: {error}",
                        path_to_string(&database_path)
                    ),
                )
            })?;

        operation(&mut connection).await
    }

    pub async fn transaction<T>(
        &self,
        operation: impl for<'a> FnOnce(&'a mut SqliteConnection) -> BoxDbFuture<'a, T> + Send + 'static,
    ) -> Result<T, TrackError>
    where
        T: Send + 'static,
    {
        self.run(move |connection| {
            Box::pin(async move {
                begin_transaction(connection).await?;

                match operation(connection).await {
                    Ok(value) => {
                        commit_transaction(connection).await?;
                        Ok(value)
                    }
                    Err(error) => {
                        rollback_transaction(connection)
                            .await
                            .map_err(|rollback_error| {
                                TrackError::new(
                                    error.code,
                                    format!(
                                        "{} The SQLite rollback also failed: {}",
                                        error, rollback_error
                                    ),
                                )
                            })?;
                        Err(error)
                    }
                }
            })
        })
        .await
    }

    fn connect_options(&self) -> Result<SqliteConnectOptions, TrackError> {
        if let Some(parent) = self.database_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not create the backend state directory at {}: {error}",
                        path_to_string(parent)
                    ),
                )
            })?;
        }

        Ok(SqliteConnectOptions::new()
            .filename(&self.database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal))
    }
}

async fn begin_transaction(connection: &mut SqliteConnection) -> Result<(), TrackError> {
    // Migration imports need all-or-nothing behavior. A plain BEGIN keeps the
    // implementation simple while still ensuring the new SQLite state either
    // fully replaces the legacy files or stays empty enough to retry safely.
    sqlx::query("BEGIN")
        .execute(&mut *connection)
        .await
        .map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not begin the SQLite transaction: {error}"),
            )
        })?;

    Ok(())
}

async fn commit_transaction(connection: &mut SqliteConnection) -> Result<(), TrackError> {
    sqlx::query("COMMIT")
        .execute(&mut *connection)
        .await
        .map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not commit the SQLite transaction: {error}"),
            )
        })?;

    Ok(())
}

async fn rollback_transaction(connection: &mut SqliteConnection) -> Result<(), TrackError> {
    sqlx::query("ROLLBACK")
        .execute(&mut *connection)
        .await
        .map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not roll back the SQLite transaction: {error}"),
            )
        })?;

    Ok(())
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
        "task_dispatches",
        "reviews",
        "review_runs",
        "backend_settings",
    ];

    #[tokio::test]
    async fn initialize_applies_the_embedded_sqlx_migration_to_a_fresh_database() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::new(Some(directory.path().join("track.sqlite")))
            .expect("database context should resolve");

        database
            .initialize()
            .await
            .expect("database schema should initialize");

        let migration_count = database
            .run(|connection| {
                Box::pin(async move {
                    let row = sqlx::query("SELECT COUNT(*) AS count FROM _sqlx_migrations")
                        .fetch_one(&mut *connection)
                        .await
                        .expect("migration count query should succeed");

                    Ok(row.get::<i64, _>("count"))
                })
            })
            .await
            .expect("migration count should load");
        assert_eq!(migration_count, 1);

        for table_name in EXPECTED_APPLICATION_TABLES {
            let exists = database
                .run(move |connection| {
                    Box::pin(async move {
                        let row = sqlx::query(
                            "SELECT 1 AS found FROM sqlite_master WHERE type = 'table' AND name = ?1",
                        )
                        .bind(table_name)
                        .fetch_optional(&mut *connection)
                        .await
                        .expect("table existence query should succeed");

                        Ok(row.is_some())
                    })
                })
                .await
                .expect("table existence should load");
            assert!(
                exists,
                "expected table {table_name} to exist after initialization"
            );
        }
    }
}
