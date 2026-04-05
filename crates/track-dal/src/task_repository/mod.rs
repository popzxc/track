mod records;

use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::{ProjectId, TaskId};
use track_types::time_utils::{format_iso_8601_millis, now_utc};
use track_types::types::{Status, StoredTask, Task, TaskCreateInput, TaskSource, TaskUpdateInput};

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct FileTaskRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> FileTaskRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    pub async fn create_task(&self, input: TaskCreateInput) -> Result<StoredTask, TrackError> {
        let input = input.validate()?;
        self.ensure_project_exists(&input.project).await?;

        let timestamp = now_utc();
        let slug_source = first_non_empty_line(&input.description).unwrap_or(&input.description);
        let mut id = TaskId::unique(timestamp, slug_source);

        // TODO: Do we need that?
        if self.task_exists(&id).await.unwrap_or(false) {
            let mut suffix = 2;
            loop {
                let candidate = TaskId::new(format!("{id}-{suffix}"))
                    .expect("generated task ids should be valid path components");
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
        let mut connection = self.database.connect().await?;
        let id = task.id.as_str();
        let project = task.project.as_str();
        let priority = task.priority.as_str();
        let status = task.status.as_str();
        let description = task.description.as_str();
        let created_at = format_iso_8601_millis(task.created_at);
        let updated_at = format_iso_8601_millis(task.updated_at);
        let source = task.source.map(task_source_as_str);
        sqlx::query!(
            r#"
            INSERT INTO tasks (id, project, priority, status, description, created_at, updated_at, source)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            id,
            project,
            priority,
            status,
            description,
            created_at,
            updated_at,
            source,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not save task {}", task.id))?;

        Ok(StoredTask { task })
    }

    pub async fn save_task(&self, task: &Task) -> Result<(), TrackError> {
        self.ensure_project_exists(&task.project).await?;
        let task = task.clone();

        let mut connection = self.database.connect().await?;
        let id = task.id.as_str();
        let project = task.project.as_str();
        let priority = task.priority.as_str();
        let status = task.status.as_str();
        let description = task.description.as_str();
        let created_at = format_iso_8601_millis(task.created_at);
        let updated_at = format_iso_8601_millis(task.updated_at);
        let source = task.source.map(task_source_as_str);
        sqlx::query!(
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
            id,
            project,
            priority,
            status,
            description,
            created_at,
            updated_at,
            source,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not import task {}", task.id))?;

        Ok(())
    }

    pub async fn list_tasks(
        &self,
        include_closed: bool,
        project: Option<&ProjectId>,
    ) -> Result<Vec<Task>, TrackError> {
        let include_closed_flag = include_closed as i64;

        let mut connection = self.database.connect().await?;
        let rows = if let Some(project) = project {
            let project_ref = project.as_str();
            sqlx::query_as!(
                records::TaskRow,
                r#"
                SELECT
                    id AS "id!",
                    project AS "project!",
                    priority AS "priority!",
                    status AS "status!",
                    description AS "description!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    source AS "source?"
                FROM tasks
                WHERE project = ?1 AND (?2 = 1 OR status = 'open')
                ORDER BY created_at DESC
                "#,
                project_ref,
                include_closed_flag,
            )
            .fetch_all(&mut *connection)
            .await
        } else {
            sqlx::query_as!(
                records::TaskRow,
                r#"
                SELECT
                    id AS "id!",
                    project AS "project!",
                    priority AS "priority!",
                    status AS "status!",
                    description AS "description!",
                    created_at AS "created_at!",
                    updated_at AS "updated_at!",
                    source AS "source?"
                FROM tasks
                WHERE (?1 = 1 OR status = 'open')
                ORDER BY created_at DESC
                "#,
                include_closed_flag,
            )
            .fetch_all(&mut *connection)
            .await
        }
        .database_error_with("Could not list tasks from SQLite")?;

        rows.into_iter().map(Task::try_from).collect()
    }

    pub async fn get_task(&self, id: &TaskId) -> Result<Task, TrackError> {
        Ok(self.find_task_by_id(id).await?.task)
    }

    pub async fn update_task(
        &self,
        id: &TaskId,
        input: TaskUpdateInput,
    ) -> Result<Task, TrackError> {
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

        let mut connection = self.database.connect().await?;
        let id = updated_task.id.as_str();
        let priority = updated_task.priority.as_str();
        let status = updated_task.status.as_str();
        let description = updated_task.description.as_str();
        let updated_at = format_iso_8601_millis(updated_task.updated_at);
        let source = updated_task.source.map(task_source_as_str);
        sqlx::query!(
            r#"
            UPDATE tasks
            SET priority = ?2, status = ?3, description = ?4, updated_at = ?5, source = ?6
            WHERE id = ?1
            "#,
            id,
            priority,
            status,
            description,
            updated_at,
            source,
        )
        .execute(&mut *connection)
        .await
        .database_error_with(format!("Could not update task {}", updated_task.id))?;

        Ok(updated_task)
    }

    pub async fn delete_task(&self, id: &TaskId) -> Result<(), TrackError> {
        let existing = self.find_task_by_id(id).await?;
        let task_id = existing.task.id;

        let mut connection = self.database.connect().await?;
        let task_id_ref = task_id.as_str();
        sqlx::query!("DELETE FROM tasks WHERE id = ?1", task_id_ref)
            .execute(&mut *connection)
            .await
            .database_error_with(format!("Could not delete task {task_id}"))?;

        Ok(())
    }

    async fn ensure_project_exists(&self, project: &ProjectId) -> Result<(), TrackError> {
        let missing_project_name = project.to_string();
        let mut connection = self.database.connect().await?;
        let project_ref = project.as_str();
        let exists = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM projects
                WHERE canonical_name = ?1
            ) AS "exists!: i64"
            "#,
            project_ref,
        )
        .fetch_one(&mut *connection)
        .await
        .database_error_with(format!("Could not verify project {project}"))?
            != 0;

        if exists {
            Ok(())
        } else {
            Err(TrackError::new(
                ErrorCode::ProjectNotFound,
                format!("Project {missing_project_name} was not found."),
            ))
        }
    }

    async fn task_exists(&self, id: &TaskId) -> Result<bool, TrackError> {
        let task_id = id.as_str();
        let mut connection = self.database.connect().await?;
        let exists = sqlx::query_scalar!(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM tasks
                WHERE id = ?1
            ) AS "exists!: i64"
            "#,
            task_id,
        )
        .fetch_one(&mut *connection)
        .await
        .database_error_with(format!("Could not check task id {task_id}"))?
            != 0;

        Ok(exists)
    }

    async fn find_task_by_id(&self, id: &TaskId) -> Result<StoredTask, TrackError> {
        let task_id = id.as_str();
        let mut connection = self.database.connect().await?;
        let row = sqlx::query_as!(
            records::TaskRow,
            r#"
            SELECT
                id AS "id!",
                project AS "project!",
                priority AS "priority!",
                status AS "status!",
                description AS "description!",
                created_at AS "created_at!",
                updated_at AS "updated_at!",
                source AS "source?"
            FROM tasks
            WHERE id = ?1
            "#,
            task_id,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!("Could not load task {task_id}"))?
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::TaskNotFound,
                format!("Task {task_id} was not found."),
            )
        })?;

        Ok(StoredTask {
            task: Task::try_from(row)?,
        })
    }
}

fn task_source_as_str(source: TaskSource) -> &'static str {
    match source {
        TaskSource::Cli => "cli",
        TaskSource::Web => "web",
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

    use crate::database::DatabaseContext;
    use crate::test_support::{
        parse_project_id, project_metadata, sample_task, temporary_database_path,
    };

    async fn database_with_projects(projects: &[&str]) -> (TempDir, DatabaseContext) {
        let (directory, database_path) = temporary_database_path();
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let project_repository = database.project_repository();

        for project in projects {
            project_repository
                .upsert_project_by_name(
                    &parse_project_id(project),
                    project_metadata(project),
                    Vec::new(),
                )
                .await
                .expect("project should save");
        }

        (directory, database)
    }

    #[tokio::test]
    async fn create_task_persists_generated_task_for_existing_project() {
        let (_directory, database) = database_with_projects(&["project-a"]).await;
        let repository = database.task_repository();

        let stored = repository
            .create_task(TaskCreateInput {
                project: parse_project_id("project-a"),
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
        let database = DatabaseContext::initialized(Some(database_path))
            .await
            .expect("database should resolve");
        let repository = database.task_repository();

        let error = repository
            .create_task(TaskCreateInput {
                project: parse_project_id("missing-project"),
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
        let (_directory, database) = database_with_projects(&["project-a"]).await;
        let repository = database.task_repository();

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
        let (_directory, database) = database_with_projects(&["project-a", "project-b"]).await;
        let repository = database.task_repository();

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
            .list_tasks(true, Some(&parse_project_id("project-a")))
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
        let (_directory, database) = database_with_projects(&["project-a"]).await;
        let repository = database.task_repository();

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
        let (_directory, database) = database_with_projects(&["project-a"]).await;
        let repository = database.task_repository();

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
