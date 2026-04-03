use sqlx::SqliteConnection;
use tempfile::TempDir;

use crate::database::DatabaseContext;

pub(super) fn temporary_database() -> (TempDir, DatabaseContext) {
    let directory = TempDir::new().expect("tempdir should be created");
    let database = DatabaseContext::new(Some(directory.path().join("track.sqlite")))
        .expect("database context should resolve");

    (directory, database)
}

pub(super) async fn execute_statements(
    connection: &mut SqliteConnection,
    statements: &[&str],
    context: &str,
) {
    for statement in statements {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .expect(context);
    }
}
