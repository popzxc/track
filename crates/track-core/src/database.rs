use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{Connection, Row, SqliteConnection};

use crate::errors::{ErrorCode, TrackError};
use crate::paths::{get_backend_database_path, path_to_string};

type BoxDbFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, TrackError>> + Send + 'a>>;

const SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS projects (
        canonical_name TEXT PRIMARY KEY,
        repo_url TEXT NOT NULL DEFAULT '',
        git_url TEXT NOT NULL DEFAULT '',
        base_branch TEXT NOT NULL DEFAULT 'main',
        description TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS project_aliases (
        canonical_name TEXT NOT NULL,
        alias TEXT NOT NULL,
        PRIMARY KEY (canonical_name, alias),
        UNIQUE (alias),
        FOREIGN KEY (canonical_name) REFERENCES projects(canonical_name) ON DELETE CASCADE
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        project TEXT NOT NULL,
        priority TEXT NOT NULL,
        status TEXT NOT NULL,
        description TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        source TEXT,
        FOREIGN KEY (project) REFERENCES projects(canonical_name) ON DELETE RESTRICT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS task_dispatches (
        dispatch_id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL,
        project TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        finished_at TEXT,
        remote_host TEXT NOT NULL,
        branch_name TEXT,
        worktree_path TEXT,
        pull_request_url TEXT,
        preferred_tool TEXT NOT NULL DEFAULT 'codex',
        follow_up_request TEXT,
        summary TEXT,
        notes TEXT,
        error_message TEXT,
        review_request_head_oid TEXT,
        review_request_user TEXT,
        FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
    )
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_task_dispatches_task_id_created_at
    ON task_dispatches(task_id, created_at DESC)
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS reviews (
        id TEXT PRIMARY KEY,
        pull_request_url TEXT NOT NULL,
        pull_request_number INTEGER NOT NULL,
        pull_request_title TEXT NOT NULL,
        repository_full_name TEXT NOT NULL,
        repo_url TEXT NOT NULL,
        git_url TEXT NOT NULL,
        base_branch TEXT NOT NULL,
        workspace_key TEXT NOT NULL,
        preferred_tool TEXT NOT NULL DEFAULT 'codex',
        project TEXT,
        main_user TEXT NOT NULL,
        default_review_prompt TEXT,
        extra_instructions TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS review_runs (
        dispatch_id TEXT PRIMARY KEY,
        review_id TEXT NOT NULL,
        pull_request_url TEXT NOT NULL,
        repository_full_name TEXT NOT NULL,
        workspace_key TEXT NOT NULL,
        preferred_tool TEXT NOT NULL DEFAULT 'codex',
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        finished_at TEXT,
        remote_host TEXT NOT NULL,
        branch_name TEXT,
        worktree_path TEXT,
        follow_up_request TEXT,
        target_head_oid TEXT,
        summary TEXT,
        review_submitted INTEGER NOT NULL DEFAULT 0,
        github_review_id TEXT,
        github_review_url TEXT,
        notes TEXT,
        error_message TEXT,
        FOREIGN KEY (review_id) REFERENCES reviews(id) ON DELETE CASCADE
    )
    "#,
    r#"
    CREATE INDEX IF NOT EXISTS idx_review_runs_review_id_created_at
    ON review_runs(review_id, created_at DESC)
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS backend_settings (
        setting_key TEXT PRIMARY KEY,
        setting_json TEXT NOT NULL
    )
    "#,
];

const ADDITIVE_SCHEMA_UPDATES: &[(&str, &str, &str)] = &[
    (
        "task_dispatches",
        "preferred_tool",
        "TEXT NOT NULL DEFAULT 'codex'",
    ),
    ("reviews", "preferred_tool", "TEXT NOT NULL DEFAULT 'codex'"),
    (
        "review_runs",
        "preferred_tool",
        "TEXT NOT NULL DEFAULT 'codex'",
    ),
];

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
            Some(database_path) => database_path.join(crate::paths::DATABASE_FILE_NAME),
            None => get_backend_database_path()?,
        };

        Ok(Self { database_path })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn initialize(&self) -> Result<(), TrackError> {
        self.run(|connection| {
            Box::pin(async move {
                for statement in SCHEMA_STATEMENTS {
                    sqlx::query(statement)
                        .execute(&mut *connection)
                        .await
                        .map_err(|error| {
                            TrackError::new(
                                ErrorCode::TaskWriteFailed,
                                format!("Could not initialize the SQLite schema: {error}"),
                            )
                        })?;
                }

                apply_additive_schema_updates(connection).await?;

                Ok(())
            })
        })
    }

    pub fn run<T>(
        &self,
        operation: impl for<'a> FnOnce(&'a mut SqliteConnection) -> BoxDbFuture<'a, T> + Send + 'static,
    ) -> Result<T, TrackError>
    where
        T: Send + 'static,
    {
        let connect_options = self.connect_options()?;
        let database_path = self.database_path.clone();

        if tokio::runtime::Handle::try_current().is_ok() {
            return std::thread::spawn(move || {
                run_database_operation(connect_options, database_path, operation)
            })
            .join()
            .map_err(|_| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    "The SQLite worker thread panicked.",
                )
            })?;
        }

        run_database_operation(connect_options, database_path, operation)
    }

    pub fn transaction<T>(
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

// =============================================================================
// SQLite Additive Migrations
// =============================================================================
//
// The backend still keeps schema setup intentionally lightweight, but new
// releases now need to add columns to already-existing local databases. We only
// support additive updates here because they are safe to apply opportunistically
// during startup without introducing a full migration framework yet.
async fn apply_additive_schema_updates(
    connection: &mut SqliteConnection,
) -> Result<(), TrackError> {
    for (table_name, column_name, column_definition) in ADDITIVE_SCHEMA_UPDATES {
        if sqlite_column_exists(connection, table_name, column_name).await? {
            continue;
        }

        let alter_statement =
            format!("ALTER TABLE {table_name} ADD COLUMN {column_name} {column_definition}");
        if let Err(error) = sqlx::query(&alter_statement)
            .execute(&mut *connection)
            .await
        {
            // Two track processes can start against the same SQLite file at the
            // same time. If both observe the old schema, one `ADD COLUMN`
            // finishes first and the other sees SQLite's duplicate-column
            // error. That outcome still means the schema is now correct, so we
            // treat it as a successful concurrent upgrade instead of aborting
            // startup.
            if sqlite_duplicate_column_error(&error, column_name) {
                continue;
            }

            return Err(TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not add the SQLite column {table_name}.{column_name}: {error}"),
            ));
        }
    }

    Ok(())
}

async fn sqlite_column_exists(
    connection: &mut SqliteConnection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, TrackError> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table_name})"))
        .fetch_all(&mut *connection)
        .await
        .map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not inspect the SQLite schema for table {table_name}: {error}"),
            )
        })?;

    Ok(rows
        .into_iter()
        .any(|row| row.get::<String, _>("name") == column_name))
}

fn sqlite_duplicate_column_error(error: &sqlx::Error, column_name: &str) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    let column_name = column_name.to_ascii_lowercase();

    message.contains("duplicate column name") && message.contains(&column_name)
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

fn run_database_operation<T>(
    connect_options: SqliteConnectOptions,
    database_path: PathBuf,
    operation: impl for<'a> FnOnce(&'a mut SqliteConnection) -> BoxDbFuture<'a, T>,
) -> Result<T, TrackError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not start the SQLite runtime: {error}"),
            )
        })?;

    runtime.block_on(async move {
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
    })
}
