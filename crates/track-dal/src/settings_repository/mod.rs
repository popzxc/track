mod records;

use serde::{de::DeserializeOwned, Serialize};
use track_types::errors::{ErrorCode, TrackError};

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct SettingsRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> SettingsRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    // TODO: strong typing is a joke I guess?
    pub async fn load_json<T>(&self, key: &str) -> Result<Option<T>, TrackError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let key = key.to_owned();
        let mut connection = self.database.connect().await?;
        let key_ref = key.as_str();
        let row = sqlx::query_as!(
            records::SettingJsonRecord,
            r#"
            SELECT setting_json AS "setting_json!"
            FROM backend_settings
            WHERE setting_key = ?1
            "#,
            key_ref,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!("Could not load backend setting `{key}`"))?;

        row.map(|row| {
            serde_json::from_str::<T>(row.setting_json.as_str()).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!("Backend setting `{key}` is not valid JSON: {error}"),
                )
            })
        })
        .transpose()
    }

    pub async fn save_json<T>(&self, key: &str, value: &T) -> Result<(), TrackError>
    where
        T: Serialize,
    {
        let key = key.to_owned();
        let serialized = serde_json::to_string(value).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not serialize backend setting `{key}`: {error}"),
            )
        })?;

        let mut connection = self.database.connect().await?;
        let key_ref = key.as_str();
        let serialized_ref = serialized.as_str();
        sqlx::query!(
            r#"
            INSERT INTO backend_settings (setting_key, setting_json)
            VALUES (?1, ?2)
            ON CONFLICT(setting_key) DO UPDATE SET setting_json = excluded.setting_json
            "#,
            key_ref,
            serialized_ref,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not save backend setting `{key}`"))?;

        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<(), TrackError> {
        let key = key.to_owned();
        let mut connection = self.database.connect().await?;
        let key_ref = key.as_str();
        sqlx::query!(
            "DELETE FROM backend_settings WHERE setting_key = ?1",
            key_ref
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not delete backend setting `{key}`"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use track_types::errors::ErrorCode;

    use crate::database::DatabaseContext;
    use crate::test_support::temporary_database_path;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct ExampleSettings {
        enabled: bool,
        retries: u32,
    }

    #[tokio::test]
    async fn save_load_and_delete_json_round_trip() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.settings_repository();

        repository
            .save_json(
                "remote-agent",
                &ExampleSettings {
                    enabled: true,
                    retries: 3,
                },
            )
            .await
            .expect("settings should save");

        let loaded = repository
            .load_json::<ExampleSettings>("remote-agent")
            .await
            .expect("settings should load");
        assert_eq!(
            loaded,
            Some(ExampleSettings {
                enabled: true,
                retries: 3,
            }),
        );

        repository
            .delete("remote-agent")
            .await
            .expect("settings should delete");

        let loaded = repository
            .load_json::<ExampleSettings>("remote-agent")
            .await
            .expect("settings should load after delete");
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn save_json_overwrites_existing_value_for_the_same_key() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.settings_repository();

        repository
            .save_json(
                "remote-agent",
                &ExampleSettings {
                    enabled: false,
                    retries: 1,
                },
            )
            .await
            .expect("initial settings should save");
        repository
            .save_json(
                "remote-agent",
                &ExampleSettings {
                    enabled: true,
                    retries: 5,
                },
            )
            .await
            .expect("updated settings should save");

        let loaded = repository
            .load_json::<ExampleSettings>("remote-agent")
            .await
            .expect("settings should load");
        assert_eq!(
            loaded,
            Some(ExampleSettings {
                enabled: true,
                retries: 5,
            }),
        );
    }

    #[tokio::test]
    async fn load_json_rejects_invalid_json_payloads() {
        let (_directory, database_path) = temporary_database_path();
        let database = DatabaseContext::uninitialized(Some(database_path))
            .await
            .expect("database should resolve");
        database
            .initialize()
            .await
            .expect("database schema should initialize");

        // The DAL pool is intentionally single-connection. Seed the invalid
        // fixture row in a narrow scope so repository construction can acquire
        // that same pooled connection again during initialization.
        {
            let mut connection = database
                .connect()
                .await
                .expect("fixture connection should open");
            sqlx::query("INSERT INTO backend_settings (setting_key, setting_json) VALUES (?1, ?2)")
                .bind("remote-agent")
                .bind("{not-json")
                .execute(&mut *connection)
                .await
                .expect("invalid fixture should insert");
        }

        let repository = database.settings_repository();
        let error = repository
            .load_json::<ExampleSettings>("remote-agent")
            .await
            .expect_err("invalid JSON should fail");

        assert_eq!(error.code, ErrorCode::InvalidConfig);
    }
}
