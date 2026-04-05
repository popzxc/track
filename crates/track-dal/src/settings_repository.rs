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
