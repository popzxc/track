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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use track_types::errors::ErrorCode;
    use track_types::time_utils::format_iso_8601_millis;
    use track_types::types::{Priority, Status, TaskCreateInput, TaskSource, TaskUpdateInput};

    use super::FileTaskRepository;
    use crate::project_repository::ProjectRepository;
    use crate::test_support::{project_metadata, sample_task, temporary_database_path};

    async fn repository_with_projects(projects: &[&str]) -> (TempDir, FileTaskRepository) {
        let (directory, database_path) = temporary_database_path();
        let project_repository = ProjectRepository::new(Some(database_path.clone()))
            .await
            .expect("project repository should resolve");

        for project in projects {
            project_repository
                .upsert_project_by_name(project, project_metadata(project), Vec::new())
                .await
                .expect("project should save");
        }

        let repository = FileTaskRepository::new(Some(database_path))
            .await
            .expect("task repository should resolve");
        (directory, repository)
    }

    #[tokio::test]
    async fn create_task_persists_generated_task_for_existing_project() {
        let (_directory, repository) = repository_with_projects(&["project-a"]).await;

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-a".to_owned(),
                priority: Priority::High,
                description: "  First line\n\nMore context  ".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .await
            .expect("task should save");

        assert_eq!(stored.task.project, "project-a");
        assert_eq!(stored.task.priority, Priority::High);
        assert_eq!(stored.task.status, Status::Open);
        assert_eq!(stored.task.description, "First line\n\nMore context");
        assert_eq!(stored.task.source, Some(TaskSource::Cli));
        assert_eq!(stored.file_path, repository.database.database_path());

        let loaded = repository
            .get_task(&stored.task.id)
            .await
            .expect("task should load");
        assert_eq!(loaded.id, stored.task.id);
        assert_eq!(loaded.project, stored.task.project);
        assert_eq!(loaded.priority, stored.task.priority);
        assert_eq!(loaded.status, stored.task.status);
        assert_eq!(loaded.description, stored.task.description);
        assert_eq!(loaded.source, stored.task.source);
        assert_eq!(
            format_iso_8601_millis(loaded.created_at),
            format_iso_8601_millis(stored.task.created_at),
        );
        assert_eq!(
            format_iso_8601_millis(loaded.updated_at),
            format_iso_8601_millis(stored.task.updated_at),
        );
    }

    #[tokio::test]
    async fn create_task_rejects_missing_project() {
        let (directory, database_path) = temporary_database_path();
        let repository = FileTaskRepository::new(Some(database_path))
            .await
            .expect("task repository should resolve");

        let error = repository
            .create_task(TaskCreateInput {
                project: "missing-project".to_owned(),
                priority: Priority::Medium,
                description: "Investigate missing project".to_owned(),
                source: Some(TaskSource::Web),
            })
            .await
            .expect_err("missing project should fail");

        drop(directory);
        assert_eq!(error.code, ErrorCode::ProjectNotFound);
    }

    #[tokio::test]
    async fn save_task_upserts_existing_rows() {
        let (_directory, repository) = repository_with_projects(&["project-a"]).await;

        let original = sample_task(
            "20260405-120000-upsert-task",
            "project-a",
            Priority::Low,
            Status::Open,
            "Original description",
            "2026-04-05T12:00:00.000Z",
            "2026-04-05T12:00:00.000Z",
            Some(TaskSource::Cli),
        );
        repository
            .save_task(&original)
            .await
            .expect("original task should save");

        let updated = sample_task(
            &original.id,
            "project-a",
            Priority::High,
            Status::Closed,
            "Updated description",
            "2026-04-05T12:00:00.000Z",
            "2026-04-05T13:00:00.000Z",
            Some(TaskSource::Web),
        );
        repository
            .save_task(&updated)
            .await
            .expect("updated task should save");

        let loaded = repository
            .get_task(&original.id)
            .await
            .expect("task should load");
        assert_eq!(loaded, updated);
    }

    #[tokio::test]
    async fn list_tasks_filters_by_project_and_closed_state() {
        let (_directory, repository) = repository_with_projects(&["project-a", "project-b"]).await;

        let project_a_open = sample_task(
            "20260405-120000-project-a-open",
            "project-a",
            Priority::High,
            Status::Open,
            "Open task in project A",
            "2026-04-05T12:00:00.000Z",
            "2026-04-05T12:00:00.000Z",
            Some(TaskSource::Cli),
        );
        let project_a_closed = sample_task(
            "20260405-130000-project-a-closed",
            "project-a",
            Priority::Medium,
            Status::Closed,
            "Closed task in project A",
            "2026-04-05T13:00:00.000Z",
            "2026-04-05T13:00:00.000Z",
            Some(TaskSource::Web),
        );
        let project_b_open = sample_task(
            "20260405-140000-project-b-open",
            "project-b",
            Priority::Low,
            Status::Open,
            "Open task in project B",
            "2026-04-05T14:00:00.000Z",
            "2026-04-05T14:00:00.000Z",
            None,
        );

        repository
            .save_task(&project_a_open)
            .await
            .expect("project a open task should save");
        repository
            .save_task(&project_a_closed)
            .await
            .expect("project a closed task should save");
        repository
            .save_task(&project_b_open)
            .await
            .expect("project b open task should save");

        let open_tasks = repository
            .list_tasks(false, None)
            .await
            .expect("open task list should load");
        assert_eq!(
            open_tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "20260405-140000-project-b-open",
                "20260405-120000-project-a-open",
            ],
        );

        let project_a_tasks = repository
            .list_tasks(true, Some("project-a"))
            .await
            .expect("project task list should load");
        assert_eq!(
            project_a_tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "20260405-130000-project-a-closed",
                "20260405-120000-project-a-open",
            ],
        );
    }

    #[tokio::test]
    async fn update_task_persists_mutable_fields() {
        let (_directory, repository) = repository_with_projects(&["project-a"]).await;

        let original = sample_task(
            "20260405-120000-update-task",
            "project-a",
            Priority::Medium,
            Status::Open,
            "Original description",
            "2026-04-04T12:00:00.000Z",
            "2026-04-04T12:00:00.000Z",
            Some(TaskSource::Cli),
        );
        repository
            .save_task(&original)
            .await
            .expect("original task should save");

        let updated = repository
            .update_task(
                &original.id,
                TaskUpdateInput {
                    description: Some("Updated description".to_owned()),
                    priority: Some(Priority::High),
                    status: Some(Status::Closed),
                },
            )
            .await
            .expect("task should update");

        assert_eq!(updated.id, original.id);
        assert_eq!(updated.description, "Updated description");
        assert_eq!(updated.priority, Priority::High);
        assert_eq!(updated.status, Status::Closed);
        assert_eq!(updated.created_at, original.created_at);
        assert!(updated.updated_at > original.updated_at);

        let loaded = repository
            .get_task(&original.id)
            .await
            .expect("updated task should load");
        assert_eq!(loaded.id, updated.id);
        assert_eq!(loaded.project, updated.project);
        assert_eq!(loaded.priority, updated.priority);
        assert_eq!(loaded.status, updated.status);
        assert_eq!(loaded.description, updated.description);
        assert_eq!(loaded.source, updated.source);
        assert_eq!(
            format_iso_8601_millis(loaded.created_at),
            format_iso_8601_millis(updated.created_at),
        );
        assert_eq!(
            format_iso_8601_millis(loaded.updated_at),
            format_iso_8601_millis(updated.updated_at),
        );
    }

    #[tokio::test]
    async fn delete_task_removes_the_row() {
        let (_directory, repository) = repository_with_projects(&["project-a"]).await;

        let task = sample_task(
            "20260405-120000-delete-task",
            "project-a",
            Priority::Low,
            Status::Open,
            "Task to delete",
            "2026-04-05T12:00:00.000Z",
            "2026-04-05T12:00:00.000Z",
            None,
        );
        repository.save_task(&task).await.expect("task should save");

        repository
            .delete_task(&task.id)
            .await
            .expect("task should delete");

        let error = repository
            .get_task(&task.id)
            .await
            .expect_err("deleted task should be missing");
        assert_eq!(error.code, ErrorCode::TaskNotFound);
    }
}
