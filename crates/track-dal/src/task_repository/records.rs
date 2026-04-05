use track_types::errors::{ErrorCode, TrackError};
use track_types::time_utils::parse_iso_8601_millis;
use track_types::types::{Priority, Status, Task, TaskSource};

#[derive(Debug, sqlx::FromRow)]
pub(super) struct TaskRow {
    pub(super) id: String,
    pub(super) project: String,
    pub(super) priority: String,
    pub(super) status: String,
    pub(super) description: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) source: Option<String>,
}

impl TryFrom<TaskRow> for Task {
    type Error = TrackError;

    fn try_from(record: TaskRow) -> Result<Self, Self::Error> {
        let id = record.id;
        let priority = parse_priority(record.priority.as_str())?;
        let status = parse_status(record.status.as_str())?;
        let created_at = parse_iso_8601_millis(&record.created_at).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Task {id} has an invalid created_at timestamp: {error}"),
            )
        })?;
        let updated_at = parse_iso_8601_millis(&record.updated_at).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Task {id} has an invalid updated_at timestamp: {error}"),
            )
        })?;
        let source = record
            .source
            .as_deref()
            .map(parse_task_source)
            .transpose()?;

        Ok(Task {
            id,
            project: record.project,
            priority,
            status,
            description: record.description,
            created_at,
            updated_at,
            source,
        })
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
