// TODO: This module serves as a temporary encapsulation boundary
// to contain the signs of the old behavior where migrations were
// manually rolled instead of using sqlx native functionality.
// After the next release, this module is to be removed.

use sqlx::migrate::{Migrate, Migrator};
use sqlx::SqliteConnection;
use track_types::errors::{ErrorCode, TrackError};

const APPLICATION_TABLES: &[&str] = &[
    "projects",
    "project_aliases",
    "tasks",
    "task_dispatches",
    "reviews",
    "review_runs",
    "backend_settings",
];

// =============================================================================
// SQLx Migration Onboarding
// =============================================================================
//
// Track shipped a handwritten SQLite bootstrap before adopting SQLx's native
// migrator. Existing user databases already contain the latest schema but lack
// SQLx's bookkeeping table, so the onboarding path has one job: recognize that
// fully initialized legacy shape, baseline the first embedded migration, and
// then let SQLx own every subsequent migration.
pub(super) async fn baseline_existing_schema_if_needed(
    connection: &mut SqliteConnection,
    migrator: &Migrator,
) -> Result<(), TrackError> {
    let existing_application_table_count = count_existing_application_tables(connection).await?;
    if existing_application_table_count == 0 {
        return Ok(());
    }

    connection
        .ensure_migrations_table()
        .await
        .map_err(|error| {
            migration_track_error("Could not prepare the SQLx migration table", error)
        })?;

    let applied_migrations = connection
        .list_applied_migrations()
        .await
        .map_err(|error| {
            migration_track_error("Could not inspect applied SQLx migrations", error)
        })?;
    if !applied_migrations.is_empty() {
        return Ok(());
    }

    if existing_application_table_count != APPLICATION_TABLES.len() {
        return Err(TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!(
                "The SQLite database has a partial pre-migration schema (found {existing_application_table_count} of {} track tables), so SQLx cannot baseline it automatically.",
                APPLICATION_TABLES.len()
            ),
        ));
    }

    let initial_migration = migrator.iter().next().ok_or_else(|| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            "The embedded SQLx migration set is empty.",
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
        VALUES (?1, ?2, TRUE, ?3, 0)
        "#,
    )
    .bind(initial_migration.version)
    .bind(&*initial_migration.description)
    .bind(initial_migration.checksum.as_ref())
    .execute(&mut *connection)
    .await
    .map_err(|error| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Could not baseline the existing SQLite schema into SQLx migrations: {error}"),
        )
    })?;

    Ok(())
}

async fn count_existing_application_tables(
    connection: &mut SqliteConnection,
) -> Result<usize, TrackError> {
    let mut count = 0;

    for table_name in APPLICATION_TABLES {
        if sqlite_table_exists(connection, table_name).await? {
            count += 1;
        }
    }

    Ok(count)
}

async fn sqlite_table_exists(
    connection: &mut SqliteConnection,
    table_name: &str,
) -> Result<bool, TrackError> {
    let row = sqlx::query(
        r#"
        SELECT 1 AS found
        FROM sqlite_master
        WHERE type = 'table' AND name = ?1
        "#,
    )
    .bind(table_name)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Could not inspect the SQLite schema for table {table_name}: {error}"),
        )
    })?;

    Ok(row.is_some())
}

fn migration_track_error(context: &str, error: sqlx::migrate::MigrateError) -> TrackError {
    TrackError::new(ErrorCode::TaskWriteFailed, format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use sqlx::Row;
    use sqlx::{Connection, SqliteConnection};
    use tempfile::TempDir;

    use crate::database::DatabaseContext;
    use crate::database::MIGRATOR;

    #[tokio::test]
    async fn baseline_existing_schema_marks_a_complete_pre_sqlx_schema_as_applied() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::new(Some(directory.path().join("track.sqlite")))
            .expect("database context should resolve");

        let connect_options = database
            .connect_options()
            .expect("database connect options should resolve");
        let mut connection = SqliteConnection::connect_with(&connect_options)
            .await
            .expect("sqlite connection should open");
        sqlx::raw_sql(include_str!("../../migrations/0001_initial_schema.sql"))
            .execute(&mut connection)
            .await
            .expect("initial schema SQL should execute");
        super::baseline_existing_schema_if_needed(&mut connection, &MIGRATOR)
            .await
            .expect("legacy schema should baseline");

        let row = sqlx::query(
            "SELECT version, success FROM _sqlx_migrations ORDER BY version ASC LIMIT 1",
        )
        .fetch_one(&mut connection)
        .await
        .expect("migration row query should succeed");
        let migration_row = (row.get::<i64, _>("version"), row.get::<i64, _>("success"));
        assert_eq!(migration_row, (1, 1));
    }
}
