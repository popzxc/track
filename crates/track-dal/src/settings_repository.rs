use serde::{de::DeserializeOwned, Serialize};
use sqlx::Row;
use track_types::errors::{ErrorCode, TrackError};

use crate::database::DatabaseContext;

#[derive(Debug, Clone)]
pub struct SettingsRepository {
    database: DatabaseContext,
}

impl SettingsRepository {
    pub async fn new(database: Option<DatabaseContext>) -> Result<Self, TrackError> {
        let database = match database {
            Some(database) => database,
            None => DatabaseContext::new(None)?,
        };
        database.initialize().await?;

        Ok(Self { database })
    }

    pub async fn load_json<T>(&self, key: &str) -> Result<Option<T>, TrackError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        let key = key.to_owned();
        self.database
            .run(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        "SELECT setting_json FROM backend_settings WHERE setting_key = ?1",
                    )
                    .bind(&key)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not load backend setting `{key}`: {error}"),
                        )
                    })?;

                    row.map(|row| {
                        serde_json::from_str::<T>(row.get::<String, _>("setting_json").as_str())
                            .map_err(|error| {
                                TrackError::new(
                                    ErrorCode::InvalidConfig,
                                    format!("Backend setting `{key}` is not valid JSON: {error}"),
                                )
                            })
                    })
                    .transpose()
                })
            })
            .await
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

        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        r#"
                    INSERT INTO backend_settings (setting_key, setting_json)
                    VALUES (?1, ?2)
                    ON CONFLICT(setting_key) DO UPDATE SET setting_json = excluded.setting_json
                    "#,
                    )
                    .bind(&key)
                    .bind(&serialized)
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not save backend setting `{key}`: {error}"),
                        )
                    })?;

                    Ok(())
                })
            })
            .await
    }

    pub async fn delete(&self, key: &str) -> Result<(), TrackError> {
        let key = key.to_owned();
        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM backend_settings WHERE setting_key = ?1")
                        .bind(&key)
                        .execute(&mut *connection)
                        .await
                        .map_err(|error| {
                            TrackError::new(
                                ErrorCode::TaskWriteFailed,
                                format!("Could not delete backend setting `{key}`: {error}"),
                            )
                        })?;

                    Ok(())
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use track_types::errors::ErrorCode;

    use super::SettingsRepository;
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
        let database = DatabaseContext::new(Some(database_path)).expect("database should resolve");
        let repository = SettingsRepository::new(Some(database))
            .await
            .expect("settings repository should resolve");

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
        let database = DatabaseContext::new(Some(database_path)).expect("database should resolve");
        let repository = SettingsRepository::new(Some(database))
            .await
            .expect("settings repository should resolve");

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
        let database = DatabaseContext::new(Some(database_path)).expect("database should resolve");
        database
            .initialize()
            .await
            .expect("database schema should initialize");
        database
            .run(|connection| {
                Box::pin(async move {
                    sqlx::query(
                        "INSERT INTO backend_settings (setting_key, setting_json) VALUES (?1, ?2)",
                    )
                    .bind("remote-agent")
                    .bind("{not-json")
                    .execute(&mut *connection)
                    .await
                    .expect("invalid fixture should insert");

                    Ok(())
                })
            })
            .await
            .expect("fixture setup should succeed");

        let repository = SettingsRepository::new(Some(database))
            .await
            .expect("settings repository should resolve");
        let error = repository
            .load_json::<ExampleSettings>("remote-agent")
            .await
            .expect_err("invalid JSON should fail");

        assert_eq!(error.code, ErrorCode::InvalidConfig);
    }
}
