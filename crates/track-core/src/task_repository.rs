use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::errors::{ErrorCode, TrackError};
use crate::paths::{get_data_dir, path_to_string};
use crate::task_id::build_unique_task_id;
use crate::time_utils::{format_iso_8601_millis, now_utc, parse_iso_8601_millis};
use crate::types::{Status, StoredTask, Task, TaskCreateInput, TaskUpdateInput};

const TASK_FILE_EXTENSION: &str = ".md";

#[derive(Debug, Serialize)]
struct TaskFrontmatter {
    priority: crate::types::Priority,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<crate::types::TaskSource>,
}

#[derive(Debug, Deserialize)]
struct ParsedTaskFrontmatter {
    priority: Option<crate::types::Priority>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    source: Option<crate::types::TaskSource>,
}

#[derive(Debug)]
struct TaskPathMetadata {
    id: String,
    project: String,
    status: Status,
}

pub struct FileTaskRepository {
    data_dir: PathBuf,
}

impl FileTaskRepository {
    pub fn new(data_dir: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            data_dir: match data_dir {
                Some(path) => path,
                None => get_data_dir()?,
            },
        })
    }

    pub fn create_task(&self, input: TaskCreateInput) -> Result<StoredTask, TrackError> {
        let trimmed_description = input.description.trim().to_owned();
        if trimmed_description.is_empty() {
            return Err(TrackError::new(
                ErrorCode::EmptyInput,
                "Please provide a task description.",
            ));
        }

        // We create project folders on demand so the first captured task can be
        // the thing that establishes a repository's task directory.
        self.ensure_project_directories(&input.project)?;

        let timestamp = now_utc();
        let destination_directory = self.get_status_directory(&input.project, Status::Open);
        let id = build_unique_task_id(timestamp, &trimmed_description, |candidate| {
            self.get_task_file_path(&input.project, Status::Open, candidate)
                .exists()
        });

        let task = Task {
            id: id.clone(),
            project: input.project.clone(),
            priority: input.priority,
            status: Status::Open,
            description: trimmed_description,
            created_at: timestamp,
            updated_at: timestamp,
            source: input.source,
        };

        let file_path = destination_directory.join(format!("{id}{TASK_FILE_EXTENSION}"));
        self.write_task_file(&file_path, &task)?;

        Ok(StoredTask { file_path, task })
    }

    pub fn list_tasks(
        &self,
        include_closed: bool,
        project: Option<&str>,
    ) -> Result<Vec<Task>, TrackError> {
        if !self.data_dir.exists() {
            return Ok(Vec::new());
        }

        let projects = match project {
            Some(project) => vec![project.to_owned()],
            None => self.list_project_directories()?,
        };
        let statuses = if include_closed {
            vec![Status::Open, Status::Closed]
        } else {
            vec![Status::Open]
        };

        let mut tasks = Vec::new();
        for project in projects {
            for status in &statuses {
                let directory_path = self.get_status_directory(&project, *status);
                if !directory_path.exists() {
                    continue;
                }

                let entries = fs::read_dir(&directory_path).map_err(|error| {
                    TrackError::new(
                        ErrorCode::TaskWriteFailed,
                        format!(
                            "Could not read the task directory at {}: {error}",
                            path_to_string(&directory_path)
                        ),
                    )
                })?;

                for entry in entries {
                    let entry = entry.map_err(|error| {
                        TrackError::new(
                            ErrorCode::TaskWriteFailed,
                            format!(
                                "Could not read a task directory entry under {}: {error}",
                                path_to_string(&directory_path)
                            ),
                        )
                    })?;

                    let file_path = entry.path();
                    let Some(file_name) = file_path.file_name().and_then(|value| value.to_str())
                    else {
                        continue;
                    };
                    if !file_name.ends_with(TASK_FILE_EXTENSION) {
                        continue;
                    }

                    match self.read_task_file(&file_path) {
                        Ok(record) => tasks.push(record.task),
                        Err(error) => {
                            eprintln!(
                                "Skipping malformed task file at {}: {}",
                                path_to_string(&file_path),
                                error
                            );
                        }
                    }
                }
            }
        }

        Ok(tasks)
    }

    pub fn update_task(&self, id: &str, input: TaskUpdateInput) -> Result<Task, TrackError> {
        let parsed_input = input.validate()?;
        let existing_record = self.find_task_by_id(id)?;

        let next_status = parsed_input.status.unwrap_or(existing_record.task.status);
        let updated_task = Task {
            description: parsed_input
                .description
                .unwrap_or(existing_record.task.description.clone()),
            priority: parsed_input
                .priority
                .unwrap_or(existing_record.task.priority),
            status: next_status,
            updated_at: now_utc(),
            ..existing_record.task.clone()
        };

        let destination_file_path =
            self.get_task_file_path(&updated_task.project, next_status, &updated_task.id);
        self.ensure_project_directories(&updated_task.project)?;
        self.write_task_file(&destination_file_path, &updated_task)?;

        if existing_record.file_path != destination_file_path {
            fs::remove_file(&existing_record.file_path).map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not remove the previous task file at {}: {error}",
                        path_to_string(&existing_record.file_path)
                    ),
                )
            })?;
        }

        Ok(updated_task)
    }

    pub fn delete_task(&self, id: &str) -> Result<(), TrackError> {
        let existing_record = self.find_task_by_id(id)?;
        fs::remove_file(&existing_record.file_path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not delete the task file at {}: {error}",
                    path_to_string(&existing_record.file_path)
                ),
            )
        })?;

        Ok(())
    }

    fn ensure_project_directories(&self, project: &str) -> Result<(), TrackError> {
        let open_directory = self.get_status_directory(project, Status::Open);
        fs::create_dir_all(&open_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not create the task directory at {} for project {project}: {error}",
                    path_to_string(&open_directory)
                ),
            )
        })?;

        let closed_directory = self.get_status_directory(project, Status::Closed);
        fs::create_dir_all(&closed_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not create the task directory at {} for project {project}: {error}",
                    path_to_string(&closed_directory)
                ),
            )
        })?;

        Ok(())
    }

    fn find_task_by_id(&self, id: &str) -> Result<StoredTask, TrackError> {
        if !self.data_dir.exists() {
            return Err(TrackError::new(
                ErrorCode::TaskNotFound,
                format!("Task {id} was not found."),
            ));
        }

        for project in self.list_project_directories()? {
            for status in [Status::Open, Status::Closed] {
                let file_path = self.get_task_file_path(&project, status, id);
                if !file_path.exists() {
                    continue;
                }

                return self.read_task_file(&file_path);
            }
        }

        Err(TrackError::new(
            ErrorCode::TaskNotFound,
            format!("Task {id} was not found."),
        ))
    }

    fn list_project_directories(&self) -> Result<Vec<String>, TrackError> {
        if !self.data_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.data_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not read the task data directory at {}: {error}",
                    path_to_string(&self.data_dir)
                ),
            )
        })?;

        let mut projects = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not read a project entry under {}: {error}",
                        path_to_string(&self.data_dir)
                    ),
                )
            })?;

            let file_type = entry.file_type().map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not inspect a project entry under {}: {error}",
                        path_to_string(&self.data_dir)
                    ),
                )
            })?;

            if !file_type.is_dir() {
                continue;
            }

            projects.push(entry.file_name().to_string_lossy().into_owned());
        }

        Ok(projects)
    }

    fn get_status_directory(&self, project: &str, status: Status) -> PathBuf {
        self.data_dir.join(project).join(status.as_str())
    }

    fn get_task_file_path(&self, project: &str, status: Status, id: &str) -> PathBuf {
        self.get_status_directory(project, status)
            .join(format!("{id}{TASK_FILE_EXTENSION}"))
    }

    fn read_task_file(&self, file_path: &Path) -> Result<StoredTask, TrackError> {
        // The file path is the stable task identity. Frontmatter no longer
        // duplicates those fields, which keeps hand-edited files from drifting
        // out of sync with the filesystem.
        let path_metadata = self.parse_task_path(file_path)?;
        let raw_file = fs::read_to_string(file_path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not read task file at {}: {error}",
                    path_to_string(file_path)
                ),
            )
        })?;
        let (frontmatter, body) = split_frontmatter(&raw_file)?;
        let parsed_frontmatter = serde_yaml::from_str::<ParsedTaskFrontmatter>(&frontmatter)
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfigInput,
                    format!("Could not parse task frontmatter: {error}"),
                )
            })?;

        let description = body.trim().to_owned();

        let task = Task {
            id: path_metadata.id,
            project: path_metadata.project,
            priority: parsed_frontmatter.priority.ok_or_else(|| {
                TrackError::new(
                    ErrorCode::InvalidConfigInput,
                    "Task frontmatter is missing required field priority.",
                )
            })?,
            status: path_metadata.status,
            description: required_body_description(description)?,
            created_at: required_timestamp(parsed_frontmatter.created_at, "createdAt")?,
            updated_at: required_timestamp(parsed_frontmatter.updated_at, "updatedAt")?,
            source: parsed_frontmatter.source,
        };

        Ok(StoredTask {
            file_path: file_path.to_path_buf(),
            task,
        })
    }

    fn parse_task_path(&self, file_path: &Path) -> Result<TaskPathMetadata, TrackError> {
        let relative_path = file_path.strip_prefix(&self.data_dir).map_err(|_| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} is outside the configured data directory.",
                    path_to_string(file_path)
                ),
            )
        })?;

        let mut components = relative_path.components();
        let project = component_as_string(components.next(), "project", file_path)?;
        let status = parse_status_component(components.next(), file_path)?;
        let file_name = component_as_string(components.next(), "task filename", file_path)?;

        if components.next().is_some() {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} does not match the expected project/status/id.md layout.",
                    path_to_string(file_path)
                ),
            ));
        }

        let Some(id) = file_name.strip_suffix(TASK_FILE_EXTENSION) else {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file {} does not use the expected {} extension.",
                    path_to_string(file_path),
                    TASK_FILE_EXTENSION
                ),
            ));
        };

        let id = id.trim();
        if id.is_empty() {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} is missing the task identifier in its filename.",
                    path_to_string(file_path)
                ),
            ));
        }

        Ok(TaskPathMetadata {
            id: id.to_owned(),
            project,
            status,
        })
    }

    fn write_task_file(&self, file_path: &Path, task: &Task) -> Result<(), TrackError> {
        let frontmatter = TaskFrontmatter {
            priority: task.priority,
            created_at: format_iso_8601_millis(task.created_at),
            updated_at: format_iso_8601_millis(task.updated_at),
            source: task.source,
        };

        let yaml = serde_yaml::to_string(&frontmatter).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not serialize task frontmatter: {error}"),
            )
        })?;
        let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);

        // Path-derived identity lives in the filesystem, and the description
        // lives in the Markdown body. Frontmatter stays focused on metadata
        // that does not already have a clearer home elsewhere.
        let serialized = format!("---\n{}---\n\n{}\n", yaml, task.description.trim());
        let temp_file_path = PathBuf::from(format!("{}.tmp", path_to_string(file_path)));

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not create the task directory for {}: {error}",
                        path_to_string(file_path)
                    ),
                )
            })?;
        }

        fs::write(&temp_file_path, serialized).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not write the temporary task file: {error}"),
            )
        })?;

        fs::rename(&temp_file_path, file_path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not move the task file into place: {error}"),
            )
        })?;

        let metadata = fs::metadata(file_path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not verify the written task file: {error}"),
            )
        })?;

        if !metadata.is_file() {
            return Err(TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not write task file at {}.",
                    path_to_string(file_path)
                ),
            ));
        }

        Ok(())
    }
}

fn required_string(value: Option<String>, field_name: &str) -> Result<String, TrackError> {
    let Some(value) = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!("Task frontmatter is missing required field {field_name}."),
        ));
    };

    Ok(value)
}

fn required_body_description(value: String) -> Result<String, TrackError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Task Markdown body is empty.",
        ));
    }

    Ok(value)
}

fn required_timestamp(
    value: Option<String>,
    field_name: &str,
) -> Result<OffsetDateTime, TrackError> {
    let value = required_string(value, field_name)?;
    parse_iso_8601_millis(&value).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!("Task frontmatter field {field_name} is not a valid timestamp: {error}"),
        )
    })
}

fn component_as_string(
    component: Option<std::path::Component<'_>>,
    label: &str,
    file_path: &Path,
) -> Result<String, TrackError> {
    let value = component
        .and_then(|component| component.as_os_str().to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} is missing the {label} component.",
                    path_to_string(file_path)
                ),
            )
        })?;

    Ok(value.to_owned())
}

fn parse_status_component(
    component: Option<std::path::Component<'_>>,
    file_path: &Path,
) -> Result<Status, TrackError> {
    let raw_status = component_as_string(component, "status", file_path)?;
    match raw_status.as_str() {
        "open" => Ok(Status::Open),
        "closed" => Ok(Status::Closed),
        _ => Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Task file path {} uses unsupported status directory {}.",
                path_to_string(file_path),
                raw_status
            ),
        )),
    }
}

fn split_frontmatter(raw_file: &str) -> Result<(String, String), TrackError> {
    // Hand-edited task files may use either LF or CRLF line endings. We treat
    // those as formatting differences, not as a reason to reject the file.
    let Some((opening_line, mut cursor)) = next_line(raw_file, 0) else {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Task file is missing YAML frontmatter.",
        ));
    };

    if trim_line_ending(opening_line) != "---" {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Task file is missing YAML frontmatter.",
        ));
    }

    let mut frontmatter = String::new();
    loop {
        let Some((line, next_cursor)) = next_line(raw_file, cursor) else {
            return Err(TrackError::new(
                ErrorCode::InvalidConfigInput,
                "Task file is missing a valid YAML frontmatter terminator.",
            ));
        };

        if trim_line_ending(line) == "---" {
            return Ok((frontmatter, raw_file[next_cursor..].to_owned()));
        }

        frontmatter.push_str(trim_line_ending(line));
        frontmatter.push('\n');
        cursor = next_cursor;
    }
}

fn next_line(raw_file: &str, cursor: usize) -> Option<(&str, usize)> {
    if cursor >= raw_file.len() {
        return None;
    }

    match raw_file[cursor..].find('\n') {
        Some(line_break) => {
            let end = cursor + line_break + 1;
            Some((&raw_file[cursor..end], end))
        }
        None => Some((&raw_file[cursor..], raw_file.len())),
    }
}

fn trim_line_ending(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::FileTaskRepository;
    use crate::errors::ErrorCode;
    use crate::types::{Priority, Status, TaskCreateInput, TaskSource};

    #[test]
    fn creates_markdown_task_files() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::High,
                description: "Fix a bug in module A".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let raw_file =
            std::fs::read_to_string(&stored.file_path).expect("task file should be readable");

        assert!(raw_file.contains("priority: high"));
        assert!(!raw_file.contains("project:"));
        assert!(!raw_file.contains("status:"));
        assert!(raw_file.contains("Fix a bug in module A"));
    }

    #[test]
    fn moves_tasks_between_status_directories_and_updates_body() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::Medium,
                description: "Investigate startup crash".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let closed_task = repository
            .update_task(
                &stored.task.id,
                crate::types::TaskUpdateInput {
                    description: None,
                    priority: None,
                    status: Some(Status::Closed),
                },
            )
            .expect("task should close");
        assert_eq!(closed_task.status, Status::Closed);

        let reopened_task = repository
            .update_task(
                &stored.task.id,
                crate::types::TaskUpdateInput {
                    description: Some("Investigate startup crash in release mode".to_owned()),
                    priority: Some(Priority::High),
                    status: Some(Status::Open),
                },
            )
            .expect("task should reopen");
        assert_eq!(reopened_task.status, Status::Open);
        assert_eq!(reopened_task.priority, Priority::High);
        assert!(reopened_task.description.contains("release mode"));
    }

    #[test]
    fn prefers_markdown_body_when_reading_tasks() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::Medium,
                description: "Original Markdown description".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let manually_edited = fs::read_to_string(&stored.file_path)
            .expect("task file should be readable")
            .replace(
                "Original Markdown description\n",
                "Edited only in the Markdown body\n",
            );
        fs::write(&stored.file_path, manually_edited).expect("manual edit should save");

        let listed_tasks = repository
            .list_tasks(true, None)
            .expect("tasks should list after a manual edit");
        assert_eq!(
            listed_tasks[0].description,
            "Edited only in the Markdown body"
        );
    }

    #[test]
    fn skips_malformed_files_during_listing() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::High,
                description: "Healthy task".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("healthy task should be created");

        let broken_task_path = directory
            .path()
            .join("issues/project-x/open/broken-task.md");
        fs::write(
            &broken_task_path,
            "---\npriority: high\n---\nThis file is missing required metadata.\n",
        )
        .expect("broken task file should be written");

        let listed_tasks = repository
            .list_tasks(true, None)
            .expect("list should skip broken files");
        assert_eq!(listed_tasks.len(), 1);
        assert_eq!(listed_tasks[0].description, "Healthy task");
    }

    #[test]
    fn ignores_unknown_identity_fields_that_are_hand_edited_into_frontmatter() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::Medium,
                description: "Keep the original location authoritative".to_owned(),
                source: Some(TaskSource::Cli),
            })
            .expect("task should be created");

        let manually_edited = fs::read_to_string(&stored.file_path)
            .expect("task file should be readable")
            .replace(
                "priority: medium\n",
                "id: hijacked-id\nproject: project-y\nstatus: closed\npriority: low\n",
            );
        fs::write(&stored.file_path, manually_edited).expect("manual edit should save");

        let listed_task = repository
            .list_tasks(true, None)
            .expect("tasks should list after a manual edit")
            .into_iter()
            .next()
            .expect("task should still exist");
        assert_eq!(listed_task.id, stored.task.id);
        assert_eq!(listed_task.project, "project-x");
        assert_eq!(listed_task.status, Status::Open);

        let updated_task = repository
            .update_task(
                &stored.task.id,
                crate::types::TaskUpdateInput {
                    description: Some("Still the same task after rewrite".to_owned()),
                    priority: None,
                    status: None,
                },
            )
            .expect("task should update without moving");

        assert_eq!(updated_task.id, stored.task.id);
        assert_eq!(updated_task.project, "project-x");
        assert_eq!(updated_task.status, Status::Open);
        assert_eq!(updated_task.priority, Priority::Low);
        assert!(stored.file_path.exists());
        assert!(!directory
            .path()
            .join("issues/project-y/closed/hijacked-id.md")
            .exists());
    }

    #[test]
    fn reads_crlf_frontmatter_files() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let task_path = directory.path().join("issues/project-x/open/manual.md");
        fs::create_dir_all(task_path.parent().expect("task path should have parent"))
            .expect("task parent should exist");
        fs::write(
            &task_path,
            "---\r\npriority: high\r\ncreatedAt: 2026-03-17T10:00:00.000Z\r\nupdatedAt: 2026-03-17T10:00:00.000Z\r\n---\r\n\r\nWindows line endings body\r\n",
        )
        .expect("task file should be written");

        let listed_task = repository
            .list_tasks(true, None)
            .expect("CRLF task file should parse")
            .into_iter()
            .next()
            .expect("task should be listed");

        assert_eq!(listed_task.id, "manual");
        assert_eq!(listed_task.project, "project-x");
        assert_eq!(listed_task.status, Status::Open);
        assert_eq!(listed_task.description, "Windows line endings body");
    }

    #[test]
    fn deletes_tasks_permanently() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = FileTaskRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");

        let stored = repository
            .create_task(TaskCreateInput {
                project: "project-x".to_owned(),
                priority: Priority::Low,
                description: "Clean up a note".to_owned(),
                source: Some(TaskSource::Web),
            })
            .expect("task should be created");

        repository
            .delete_task(&stored.task.id)
            .expect("delete should succeed");

        let error = repository
            .delete_task(&stored.task.id)
            .expect_err("deleting twice should fail");
        assert_eq!(error.code, ErrorCode::TaskNotFound);
    }
}
