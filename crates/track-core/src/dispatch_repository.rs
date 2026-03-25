use std::fs;
use std::path::PathBuf;

use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;
use crate::paths::{get_dispatches_dir, path_to_string};
use crate::time_utils::now_utc;
use crate::types::{DispatchStatus, Task, TaskDispatchRecord};

pub struct DispatchRepository {
    dispatches_dir: PathBuf,
}

impl DispatchRepository {
    pub fn new(dispatches_dir: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            dispatches_dir: match dispatches_dir {
                Some(dispatches_dir) => dispatches_dir,
                None => get_dispatches_dir()?,
            },
        })
    }

    pub fn create_dispatch(
        &self,
        task: &Task,
        remote_host: &str,
    ) -> Result<TaskDispatchRecord, TrackError> {
        let timestamp = now_utc();
        let dispatch_id = format!("dispatch-{}", timestamp.unix_timestamp_nanos());
        let record = TaskDispatchRecord {
            dispatch_id,
            task_id: task.id.clone(),
            project: task.project.clone(),
            status: DispatchStatus::Preparing,
            created_at: timestamp,
            updated_at: timestamp,
            finished_at: None,
            remote_host: remote_host.to_owned(),
            branch_name: None,
            worktree_path: None,
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };

        self.save_dispatch(&record)?;
        Ok(record)
    }

    pub fn save_dispatch(&self, record: &TaskDispatchRecord) -> Result<(), TrackError> {
        let task_dispatch_directory = self.dispatch_directory_for_task(&record.task_id)?;
        fs::create_dir_all(&task_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not create the dispatch directory at {}: {error}",
                    path_to_string(&task_dispatch_directory)
                ),
            )
        })?;

        let serialized = serde_json::to_string_pretty(record).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Could not serialize the dispatch record: {error}"),
            )
        })?;

        fs::write(
            self.dispatch_record_path(&record.task_id, &record.dispatch_id)?,
            serialized,
        )
        .map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not write the dispatch record for task {}: {error}",
                    record.task_id
                ),
            )
        })
    }

    pub fn latest_dispatch_for_task(
        &self,
        task_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        Ok(self.dispatches_for_task(task_id)?.into_iter().next())
    }

    pub fn dispatches_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let task_dispatch_directory = self.dispatch_directory_for_task(task_id)?;
        if !task_dispatch_directory.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&task_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the dispatch directory at {}: {error}",
                    path_to_string(&task_dispatch_directory)
                ),
            )
        })?;

        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a dispatch entry under {}: {error}",
                        path_to_string(&task_dispatch_directory)
                    ),
                )
            })?;

            if !entry.path().is_file() {
                continue;
            }

            let raw_record = fs::read_to_string(entry.path()).map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read the dispatch record at {}: {error}",
                        path_to_string(&entry.path())
                    ),
                )
            })?;
            let record =
                serde_json::from_str::<TaskDispatchRecord>(&raw_record).map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Dispatch record at {} is not valid JSON: {error}",
                            path_to_string(&entry.path())
                        ),
                    )
                })?;

            records.push(record);
        }

        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(records)
    }

    pub fn latest_dispatches_for_tasks(
        &self,
        task_ids: &[String],
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        let mut records = Vec::new();
        for task_id in task_ids {
            if let Some(record) = self.latest_dispatch_for_task(task_id)? {
                records.push(record);
            }
        }

        Ok(records)
    }

    // =============================================================================
    // Global Dispatch History
    // =============================================================================
    //
    // The frontend now has dedicated "Runs" surfaces, so it needs a
    // chronological view across tasks instead of only "latest dispatch per
    // task". We keep that scan in the repository so API handlers can ask for a
    // simple sorted list without duplicating filesystem traversal rules.
    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<TaskDispatchRecord>, TrackError> {
        if !self.dispatches_dir.exists() {
            return Ok(Vec::new());
        }

        let task_directories = fs::read_dir(&self.dispatches_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the dispatch root at {}: {error}",
                    path_to_string(&self.dispatches_dir)
                ),
            )
        })?;

        let mut records = Vec::new();
        for task_directory in task_directories {
            let task_directory = task_directory.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a task dispatch directory under {}: {error}",
                        path_to_string(&self.dispatches_dir)
                    ),
                )
            })?;

            if !task_directory.path().is_dir() {
                continue;
            }

            let dispatch_entries = fs::read_dir(task_directory.path()).map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read the task dispatch directory at {}: {error}",
                        path_to_string(&task_directory.path())
                    ),
                )
            })?;

            for dispatch_entry in dispatch_entries {
                let dispatch_entry = dispatch_entry.map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not read a dispatch entry under {}: {error}",
                            path_to_string(&task_directory.path())
                        ),
                    )
                })?;

                if !dispatch_entry.path().is_file() {
                    continue;
                }

                let raw_record = fs::read_to_string(dispatch_entry.path()).map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not read the dispatch record at {}: {error}",
                            path_to_string(&dispatch_entry.path())
                        ),
                    )
                })?;
                let record =
                    serde_json::from_str::<TaskDispatchRecord>(&raw_record).map_err(|error| {
                        TrackError::new(
                            ErrorCode::DispatchWriteFailed,
                            format!(
                                "Dispatch record at {} is not valid JSON: {error}",
                                path_to_string(&dispatch_entry.path())
                            ),
                        )
                    })?;
                records.push(record);
            }
        }

        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if let Some(limit) = limit {
            records.truncate(limit);
        }

        Ok(records)
    }

    // =============================================================================
    // Dispatch History Discovery
    // =============================================================================
    //
    // Manual cleanup needs to reason about task ids that still have saved
    // dispatch state even when the task file itself is gone. Surfacing that
    // scan here keeps the filesystem traversal rules in one place instead of
    // duplicating "what counts as a task history directory?" in higher layers.
    pub fn task_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        if !self.dispatches_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.dispatches_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the dispatch root at {}: {error}",
                    path_to_string(&self.dispatches_dir)
                ),
            )
        })?;

        let mut task_ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a task dispatch directory under {}: {error}",
                        path_to_string(&self.dispatches_dir)
                    ),
                )
            })?;

            if !entry
                .file_type()
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not inspect a dispatch history entry under {}: {error}",
                            path_to_string(&self.dispatches_dir)
                        ),
                    )
                })?
                .is_dir()
            {
                continue;
            }

            let task_id = entry.file_name().to_string_lossy().into_owned();
            let validated_task_id = match validate_single_normal_path_component(
                &task_id,
                "Task id",
                ErrorCode::InvalidPathComponent,
            ) {
                Ok(task_id) => task_id,
                Err(error) => {
                    eprintln!(
                        "Skipping invalid dispatch history directory {}: {}",
                        path_to_string(&entry.path()),
                        error
                    );
                    continue;
                }
            };

            task_ids.push(validated_task_id);
        }

        task_ids.sort();
        Ok(task_ids)
    }

    pub fn get_dispatch(
        &self,
        task_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<TaskDispatchRecord>, TrackError> {
        let dispatch_record_path = self.dispatch_record_path(task_id, dispatch_id)?;
        if !dispatch_record_path.exists() {
            return Ok(None);
        }

        let raw_record = fs::read_to_string(&dispatch_record_path).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the dispatch record at {}: {error}",
                    path_to_string(&dispatch_record_path)
                ),
            )
        })?;
        let record = serde_json::from_str::<TaskDispatchRecord>(&raw_record).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Dispatch record at {} is not valid JSON: {error}",
                    path_to_string(&dispatch_record_path)
                ),
            )
        })?;

        Ok(Some(record))
    }

    pub fn delete_dispatch_history_for_task(&self, task_id: &str) -> Result<(), TrackError> {
        let task_dispatch_directory = self.dispatch_directory_for_task(task_id)?;
        if !task_dispatch_directory.exists() {
            return Ok(());
        }

        fs::remove_dir_all(&task_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not remove the dispatch history at {}: {error}",
                    path_to_string(&task_dispatch_directory)
                ),
            )
        })
    }

    fn dispatch_directory_for_task(&self, task_id: &str) -> Result<PathBuf, TrackError> {
        let task_id =
            validate_single_normal_path_component(task_id, "Task id", ErrorCode::InvalidPathComponent)?;

        Ok(self.dispatches_dir.join(task_id))
    }

    fn dispatch_record_path(
        &self,
        task_id: &str,
        dispatch_id: &str,
    ) -> Result<PathBuf, TrackError> {
        let dispatch_id = validate_single_normal_path_component(
            dispatch_id,
            "Dispatch id",
            ErrorCode::InvalidPathComponent,
        )?;

        Ok(self
            .dispatch_directory_for_task(task_id)?
            .join(format!("{dispatch_id}.json")))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::DispatchRepository;
    use time::Duration;

    use crate::errors::ErrorCode;
    use crate::time_utils::now_utc;
    use crate::types::{DispatchStatus, Priority, Task, TaskDispatchRecord};

    #[test]
    fn returns_the_latest_dispatch_for_a_task() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");
        let created_at = now_utc();
        let task = Task {
            id: "task-1".to_owned(),
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            status: crate::types::Status::Open,
            description: "Investigate a failing test".to_owned(),
            created_at,
            updated_at: created_at,
            source: None,
        };

        let mut first = repository
            .create_dispatch(&task, "192.0.2.25")
            .expect("first dispatch should be created");
        assert_eq!(first.status, DispatchStatus::Preparing);
        assert_eq!(first.finished_at, None);
        first.status = DispatchStatus::Failed;
        first.finished_at = Some(created_at);
        repository
            .save_dispatch(&first)
            .expect("first dispatch should save");

        let second = TaskDispatchRecord {
            dispatch_id: "dispatch-2".to_owned(),
            task_id: task.id.clone(),
            project: task.project.clone(),
            status: DispatchStatus::Running,
            created_at: created_at + Duration::seconds(1),
            updated_at: created_at + Duration::seconds(1),
            finished_at: None,
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track/task-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/worktrees/task-1".to_owned()),
            pull_request_url: None,
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        repository
            .save_dispatch(&second)
            .expect("second dispatch should save");

        let latest = repository
            .latest_dispatch_for_task(&task.id)
            .expect("latest dispatch should load")
            .expect("task should have a dispatch");

        assert_eq!(latest.dispatch_id, "dispatch-2");
        assert_eq!(latest.status, DispatchStatus::Running);
    }

    #[test]
    fn rejects_task_ids_that_are_not_single_path_components() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");

        let error = repository
            .latest_dispatch_for_task("../escape")
            .expect_err("dispatch lookup should reject traversal-shaped task ids");

        assert_eq!(error.code, ErrorCode::InvalidPathComponent);
    }

    #[test]
    fn rejects_dispatch_ids_that_are_not_single_path_components() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");
        let created_at = now_utc();

        let error = repository
            .save_dispatch(&TaskDispatchRecord {
                dispatch_id: "../dispatch-2".to_owned(),
                task_id: "task-1".to_owned(),
                project: "project-a".to_owned(),
                status: DispatchStatus::Running,
                created_at,
                updated_at: created_at,
                finished_at: None,
                remote_host: "192.0.2.25".to_owned(),
                branch_name: None,
                worktree_path: None,
                pull_request_url: None,
                follow_up_request: None,
                summary: None,
                notes: None,
                error_message: None,
                review_request_head_oid: None,
                review_request_user: None,
            })
            .expect_err("dispatch writes should reject traversal-shaped dispatch ids");

        assert_eq!(error.code, ErrorCode::InvalidPathComponent);
    }

    #[test]
    fn lists_dispatch_history_newest_first() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");
        let created_at = now_utc();
        let task = Task {
            id: "task-1".to_owned(),
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            status: crate::types::Status::Open,
            description: "Investigate a failing test".to_owned(),
            created_at,
            updated_at: created_at,
            source: None,
        };

        let mut older = repository
            .create_dispatch(&task, "192.0.2.25")
            .expect("older dispatch should be created");
        older.status = DispatchStatus::Failed;
        older.created_at = created_at;
        older.updated_at = created_at;
        repository
            .save_dispatch(&older)
            .expect("older dispatch should save");

        let newer = TaskDispatchRecord {
            dispatch_id: "dispatch-2".to_owned(),
            task_id: task.id.clone(),
            project: task.project.clone(),
            status: DispatchStatus::Succeeded,
            created_at: created_at + Duration::seconds(1),
            updated_at: created_at + Duration::seconds(1),
            finished_at: Some(created_at + Duration::seconds(2)),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track/task-1".to_owned()),
            worktree_path: Some("~/workspace/project-a/worktrees/task-1".to_owned()),
            pull_request_url: Some("https://github.com/acme/project-a/pull/1".to_owned()),
            follow_up_request: None,
            summary: None,
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        repository
            .save_dispatch(&newer)
            .expect("newer dispatch should save");

        let records = repository
            .dispatches_for_task(&task.id)
            .expect("dispatch history should load");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].dispatch_id, "dispatch-2");
        assert_eq!(records[1].dispatch_id, older.dispatch_id);
    }

    #[test]
    fn loads_and_deletes_dispatch_history_for_a_task() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");
        let created_at = now_utc();
        let task = Task {
            id: "task-1".to_owned(),
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            status: crate::types::Status::Open,
            description: "Investigate a failing test".to_owned(),
            created_at,
            updated_at: created_at,
            source: None,
        };

        let created = repository
            .create_dispatch(&task, "192.0.2.25")
            .expect("dispatch should be created");

        let loaded = repository
            .get_dispatch(&task.id, &created.dispatch_id)
            .expect("dispatch should load")
            .expect("dispatch should exist");
        assert_eq!(loaded.dispatch_id, created.dispatch_id);

        repository
            .delete_dispatch_history_for_task(&task.id)
            .expect("dispatch history should delete");

        assert!(
            repository
                .latest_dispatch_for_task(&task.id)
                .expect("latest dispatch lookup should succeed")
                .is_none()
        );
    }

    #[test]
    fn lists_dispatches_across_tasks_in_reverse_chronological_order() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = DispatchRepository::new(Some(directory.path().join(".dispatches")))
            .expect("dispatch repository should resolve");
        let created_at = now_utc();

        let first_task = Task {
            id: "task-1".to_owned(),
            project: "project-a".to_owned(),
            priority: Priority::Medium,
            status: crate::types::Status::Open,
            description: "Investigate a failing test".to_owned(),
            created_at,
            updated_at: created_at,
            source: None,
        };
        let second_task = Task {
            id: "task-2".to_owned(),
            project: "project-b".to_owned(),
            priority: Priority::High,
            status: crate::types::Status::Open,
            description: "Fix a release blocker".to_owned(),
            created_at,
            updated_at: created_at,
            source: None,
        };

        let mut older = repository
            .create_dispatch(&first_task, "192.0.2.25")
            .expect("older dispatch should be created");
        older.created_at = created_at;
        older.updated_at = created_at;
        repository
            .save_dispatch(&older)
            .expect("older dispatch should save");

        let newer = TaskDispatchRecord {
            dispatch_id: "dispatch-2".to_owned(),
            task_id: second_task.id.clone(),
            project: second_task.project.clone(),
            status: DispatchStatus::Succeeded,
            created_at: created_at + Duration::seconds(1),
            updated_at: created_at + Duration::seconds(1),
            finished_at: Some(created_at + Duration::seconds(2)),
            remote_host: "192.0.2.25".to_owned(),
            branch_name: Some("track/task-2".to_owned()),
            worktree_path: Some("~/workspace/project-b/worktrees/task-2".to_owned()),
            pull_request_url: Some("https://github.com/acme/project-b/pull/1".to_owned()),
            follow_up_request: None,
            summary: Some("Opened a PR.".to_owned()),
            notes: None,
            error_message: None,
            review_request_head_oid: None,
            review_request_user: None,
        };
        repository
            .save_dispatch(&newer)
            .expect("newer dispatch should save");

        let listed = repository
            .list_dispatches(None)
            .expect("dispatch listing should succeed");

        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].dispatch_id, "dispatch-2");
        assert_eq!(listed[1].dispatch_id, older.dispatch_id);
    }
}
