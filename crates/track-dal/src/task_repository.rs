use std::path::PathBuf;

use sqlx::Row;
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;
use track_types::task_id::build_unique_task_id;
use track_types::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_millis};
use track_types::types::{
    Priority, Status, StoredTask, Task, TaskCreateInput, TaskSource, TaskUpdateInput,
};

use crate::database::DatabaseContext;

#[derive(Debug, Clone)]
pub struct FileTaskRepository {
    database: DatabaseContext,
}

impl FileTaskRepository {
    pub async fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize().await?;

        Ok(Self { database })
    }

    pub async fn create_task(&self, input: TaskCreateInput) -> Result<StoredTask, TrackError> {
        let input = input.validate()?;
        self.ensure_project_exists(&input.project).await?;

        let timestamp = now_utc();
        let slug_source = first_non_empty_line(&input.description).unwrap_or(&input.description);
        let mut id = build_unique_task_id(timestamp, slug_source);

        // TODO: Do we need that?
        if self.task_exists(&id).await.unwrap_or(false) {
            let mut suffix = 2;
            loop {
                let candidate = format!("{id}-{suffix}");
                // TODO: Why the hell we `unwrap_or(false)` here?
                if !self.task_exists(&candidate).await.unwrap_or(false) {
                    id = candidate;
                    break;
                }
                suffix += 1;
            }
        }

        let task = Task {
            id: id.clone(),
            project: input.project.clone(),
            priority: input.priority,
            status: Status::Open,
            description: input.description.clone(),
            created_at: timestamp,
            updated_at: timestamp,
            source: input.source,
        };
        let storage_path = self.database.database_path().to_path_buf();

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    r#"
                    INSERT INTO tasks (id, project, priority, status, description, created_at, updated_at, source)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    "#,
                )
                .bind(&task.id)
                .bind(&task.project)
                .bind(task.priority.as_str())
                .bind(task.status.as_str())
                .bind(&task.description)
                .bind(format_iso_8601_millis(task.created_at))
                .bind(format_iso_8601_millis(task.updated_at))
                .bind(task.source.map(task_source_as_str))
                .execute(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!("Could not save task {}: {error}", task.id),
                    )
                })?;

                Ok(StoredTask {
                    file_path: storage_path,
                    task,
                })
            })
        }).await
    }

    pub async fn save_task(&self, task: &Task) -> Result<(), TrackError> {
        self.ensure_project_exists(&task.project).await?;
        let task = task.clone();

        self.database.run(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    r#"
                    INSERT INTO tasks (id, project, priority, status, description, created_at, updated_at, source)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT(id) DO UPDATE SET
                        project = excluded.project,
                        priority = excluded.priority,
                        status = excluded.status,
                        description = excluded.description,
                        created_at = excluded.created_at,
                        updated_at = excluded.updated_at,
                        source = excluded.source
                    "#,
                )
                .bind(&task.id)
                .bind(&task.project)
                .bind(task.priority.as_str())
                .bind(task.status.as_str())
                .bind(&task.description)
                .bind(format_iso_8601_millis(task.created_at))
                .bind(format_iso_8601_millis(task.updated_at))
                .bind(task.source.map(task_source_as_str))
                .execute(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!("Could not import task {}: {error}", task.id),
                    )
                })?;

                Ok(())
            })
        }).await
    }

    pub async fn list_tasks(
        &self,
        include_closed: bool,
        project: Option<&str>,
    ) -> Result<Vec<Task>, TrackError> {
        let project = project
            .map(|project| {
                validate_single_normal_path_component(
                    project,
                    "Task project",
                    ErrorCode::InvalidPathComponent,
                )
            })
            .transpose()?;
        let include_closed_flag = include_closed;

        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = if let Some(project) = project {
                    sqlx::query(
                        r#"
                        SELECT id, project, priority, status, description, created_at, updated_at, source
                        FROM tasks
                        WHERE project = ?1 AND (?2 = 1 OR status = 'open')
                        ORDER BY created_at DESC
                        "#,
                    )
                    .bind(project)
                    .bind(include_closed_flag as i64)
                    .fetch_all(&mut *connection)
                    .await
                } else {
                    sqlx::query(
                        r#"
                        SELECT id, project, priority, status, description, created_at, updated_at, source
                        FROM tasks
                        WHERE (?1 = 1 OR status = 'open')
                        ORDER BY created_at DESC
                        "#,
                    )
                    .bind(include_closed_flag as i64)
                    .fetch_all(&mut *connection)
                    .await
                }
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!("Could not list tasks from SQLite: {error}"),
                    )
                })?;

                rows.into_iter().map(task_from_row).collect()
            })
        }).await
    }

    pub async fn get_task(&self, id: &str) -> Result<Task, TrackError> {
        Ok(self.find_task_by_id(id).await?.task)
    }

    pub async fn update_task(&self, id: &str, input: TaskUpdateInput) -> Result<Task, TrackError> {
        let validated_input = input.validate()?;
        let existing_record = self.find_task_by_id(id).await?;
        let next_status = validated_input
            .status
            .unwrap_or(existing_record.task.status);
        let updated_task = Task {
            description: validated_input
                .description
                .unwrap_or(existing_record.task.description.clone()),
            priority: validated_input
                .priority
                .unwrap_or(existing_record.task.priority),
            status: next_status,
            updated_at: now_utc(),
            ..existing_record.task
        };

        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        r#"
                    UPDATE tasks
                    SET priority = ?2, status = ?3, description = ?4, updated_at = ?5, source = ?6
                    WHERE id = ?1
                    "#,
                    )
                    .bind(&updated_task.id)
                    .bind(updated_task.priority.as_str())
                    .bind(updated_task.status.as_str())
                    .bind(&updated_task.description)
                    .bind(format_iso_8601_millis(updated_task.updated_at))
                    .bind(updated_task.source.map(task_source_as_str))
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!("Could not update task {}: {error}", updated_task.id),
                        )
                    })?;

                    Ok(updated_task)
                })
            })
            .await
    }

    pub async fn delete_task(&self, id: &str) -> Result<(), TrackError> {
        let existing = self.find_task_by_id(id).await?;
        let task_id = existing.task.id;

        self.database
            .run(move |connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM tasks WHERE id = ?1")
                        .bind(&task_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(|error| {
                            TrackError::new(
                                ErrorCode::TaskWriteFailed,
                                format!("Could not delete task {task_id}: {error}"),
                            )
                        })?;

                    Ok(())
                })
            })
            .await
    }

    async fn ensure_project_exists(&self, project: &str) -> Result<(), TrackError> {
        let project = validate_single_normal_path_component(
            project,
            "Task project",
            ErrorCode::InvalidPathComponent,
        )?;

        let missing_project_name = project.clone();
        let exists = self
            .database
            .run(move |connection| {
                Box::pin(async move {
                    let row =
                        sqlx::query("SELECT 1 AS found FROM projects WHERE canonical_name = ?1")
                            .bind(&project)
                            .fetch_optional(&mut *connection)
                            .await
                            .map_err(|error| {
                                TrackError::new(
                                    ErrorCode::ProjectWriteFailed,
                                    format!("Could not verify project {project}: {error}"),
                                )
                            })?;

                    Ok(row.is_some())
                })
            })
            .await?;

        if exists {
            Ok(())
        } else {
            Err(TrackError::new(
                ErrorCode::ProjectNotFound,
                format!("Project {missing_project_name} was not found."),
            ))
        }
    }

    async fn task_exists(&self, id: &str) -> Result<bool, TrackError> {
        let task_id =
            validate_single_normal_path_component(id, "Task id", ErrorCode::InvalidPathComponent)?;
        self.database
            .run(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query("SELECT 1 AS found FROM tasks WHERE id = ?1")
                        .bind(&task_id)
                        .fetch_optional(&mut *connection)
                        .await
                        .map_err(|error| {
                            TrackError::new(
                                ErrorCode::TaskWriteFailed,
                                format!("Could not check task id {task_id}: {error}"),
                            )
                        })?;

                    Ok(row.is_some())
                })
            })
            .await
    }

    async fn find_task_by_id(&self, id: &str) -> Result<StoredTask, TrackError> {
        let task_id =
            validate_single_normal_path_component(id, "Task id", ErrorCode::InvalidPathComponent)?;
        let storage_path = self.database.database_path().to_path_buf();

        self.database.run(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    r#"
                    SELECT id, project, priority, status, description, created_at, updated_at, source
                    FROM tasks
                    WHERE id = ?1
                    "#,
                )
                .bind(&task_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!("Could not load task {task_id}: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    TrackError::new(
                        ErrorCode::TaskNotFound,
                        format!("Task {task_id} was not found."),
                    )
                })?;

                Ok(StoredTask {
                    file_path: storage_path,
                    task: task_from_row(row)?,
                })
            })
        }).await
    }
}

fn task_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Task, TrackError> {
    let id = row.get::<String, _>("id");
    let project = row.get::<String, _>("project");
    let priority = parse_priority(row.get::<String, _>("priority").as_str())?;
    let status = parse_status(row.get::<String, _>("status").as_str())?;
    let description = row.get::<String, _>("description");
    let created_at =
        parse_iso_8601_millis(&row.get::<String, _>("created_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Task {id} has an invalid created_at timestamp: {error}"),
            )
        })?;
    let updated_at =
        parse_iso_8601_millis(&row.get::<String, _>("updated_at")).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Task {id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
    let source = row
        .get::<Option<String>, _>("source")
        .as_deref()
        .map(parse_task_source)
        .transpose()?;

    Ok(Task {
        id,
        project,
        priority,
        status,
        description,
        created_at,
        updated_at,
        source,
    })
}

fn task_source_as_str(source: TaskSource) -> &'static str {
    match source {
        TaskSource::Cli => "cli",
        TaskSource::Web => "web",
    }
}

fn parse_priority(value: &str) -> Result<Priority, TrackError> {
    match value {
        "high" => Ok(Priority::High),
        "medium" => Ok(Priority::Medium),
        "low" => Ok(Priority::Low),
        _ => Err(TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Task priority `{value}` is not valid."),
        )),
    }
}

fn parse_status(value: &str) -> Result<Status, TrackError> {
    match value {
        "open" => Ok(Status::Open),
        "closed" => Ok(Status::Closed),
        _ => Err(TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Task status `{value}` is not valid."),
        )),
    }
}

fn parse_task_source(value: &str) -> Result<TaskSource, TrackError> {
    match value {
        "cli" => Ok(TaskSource::Cli),
        "web" => Ok(TaskSource::Web),
        _ => Err(TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!("Task source `{value}` is not valid."),
        )),
    }
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}
