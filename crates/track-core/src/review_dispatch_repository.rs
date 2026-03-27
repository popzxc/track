use std::fs;
use std::path::PathBuf;

use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;
use crate::paths::{get_review_dispatches_dir, path_to_string};
use crate::time_utils::now_utc;
use crate::types::{DispatchStatus, ReviewRecord, ReviewRunRecord};

pub struct ReviewDispatchRepository {
    dispatches_dir: PathBuf,
}

impl ReviewDispatchRepository {
    pub fn new(dispatches_dir: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            dispatches_dir: match dispatches_dir {
                Some(dispatches_dir) => dispatches_dir,
                None => get_review_dispatches_dir()?,
            },
        })
    }

    pub fn create_dispatch(
        &self,
        review: &ReviewRecord,
        remote_host: &str,
    ) -> Result<ReviewRunRecord, TrackError> {
        let timestamp = now_utc();
        let record = ReviewRunRecord {
            dispatch_id: format!("dispatch-{}", timestamp.unix_timestamp_nanos()),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            status: DispatchStatus::Preparing,
            created_at: timestamp,
            updated_at: timestamp,
            finished_at: None,
            remote_host: remote_host.to_owned(),
            branch_name: None,
            worktree_path: None,
            summary: None,
            review_submitted: false,
            github_review_id: None,
            github_review_url: None,
            notes: None,
            error_message: None,
        };

        self.save_dispatch(&record)?;
        Ok(record)
    }

    pub fn save_dispatch(&self, record: &ReviewRunRecord) -> Result<(), TrackError> {
        let review_dispatch_directory = self.dispatch_directory_for_review(&record.review_id)?;
        fs::create_dir_all(&review_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not create the review dispatch directory at {}: {error}",
                    path_to_string(&review_dispatch_directory)
                ),
            )
        })?;

        let serialized = serde_json::to_string_pretty(record).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!("Could not serialize the review run record: {error}"),
            )
        })?;
        let dispatch_record_path =
            self.dispatch_record_path(&record.review_id, &record.dispatch_id)?;
        let temp_record_path = review_dispatch_directory.join(format!(
            ".{}.tmp-{}",
            record.dispatch_id,
            now_utc().unix_timestamp_nanos(),
        ));

        // Write review-run snapshots through a temporary file and then rename
        // into place so readers never observe a partially rewritten JSON file
        // while the web UI is polling active review history.
        fs::write(&temp_record_path, format!("{serialized}\n")).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not write the temporary review run record for review {}: {error}",
                    record.review_id
                ),
            )
        })?;
        fs::rename(&temp_record_path, dispatch_record_path).map_err(|error| {
            let _ = fs::remove_file(&temp_record_path);
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not finalize the review run record for review {}: {error}",
                    record.review_id
                ),
            )
        })
    }

    pub fn latest_dispatch_for_review(
        &self,
        review_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        Ok(self.dispatches_for_review(review_id)?.into_iter().next())
    }

    pub fn dispatches_for_review(
        &self,
        review_id: &str,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        let review_dispatch_directory = self.dispatch_directory_for_review(review_id)?;
        if !review_dispatch_directory.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&review_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the review dispatch directory at {}: {error}",
                    path_to_string(&review_dispatch_directory)
                ),
            )
        })?;

        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a review dispatch entry under {}: {error}",
                        path_to_string(&review_dispatch_directory)
                    ),
                )
            })?;

            if !entry.path().is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
            {
                continue;
            }

            match self.read_dispatch_record_path(&entry.path()) {
                Ok(record) => records.push(record),
                Err(error) => {
                    eprintln!(
                        "Skipping malformed review run record at {}: {}",
                        path_to_string(&entry.path()),
                        error
                    );
                }
            }
        }

        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(records)
    }

    pub fn list_dispatches(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<ReviewRunRecord>, TrackError> {
        if !self.dispatches_dir.exists() {
            return Ok(Vec::new());
        }

        let review_directories = fs::read_dir(&self.dispatches_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the review dispatch root at {}: {error}",
                    path_to_string(&self.dispatches_dir)
                ),
            )
        })?;

        let mut records = Vec::new();
        for review_directory in review_directories {
            let review_directory = review_directory.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a review dispatch directory under {}: {error}",
                        path_to_string(&self.dispatches_dir)
                    ),
                )
            })?;

            if !review_directory.path().is_dir() {
                continue;
            }

            let dispatch_entries = fs::read_dir(review_directory.path()).map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read the review dispatch directory at {}: {error}",
                        path_to_string(&review_directory.path())
                    ),
                )
            })?;

            for dispatch_entry in dispatch_entries {
                let dispatch_entry = dispatch_entry.map_err(|error| {
                    TrackError::new(
                        ErrorCode::DispatchWriteFailed,
                        format!(
                            "Could not read a review run entry under {}: {error}",
                            path_to_string(&review_directory.path())
                        ),
                    )
                })?;

                if !dispatch_entry.path().is_file()
                    || dispatch_entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        != Some("json")
                {
                    continue;
                }

                match self.read_dispatch_record_path(&dispatch_entry.path()) {
                    Ok(record) => records.push(record),
                    Err(error) => {
                        eprintln!(
                            "Skipping malformed review run record at {}: {}",
                            path_to_string(&dispatch_entry.path()),
                            error
                        );
                    }
                }
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
    // Global remote cleanup needs the same "which review ids still have local
    // run history?" scan as task dispatches. Keeping the filesystem traversal
    // here avoids teaching higher layers what counts as a persisted review
    // history directory.
    pub fn review_ids_with_history(&self) -> Result<Vec<String>, TrackError> {
        if !self.dispatches_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.dispatches_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the review dispatch root at {}: {error}",
                    path_to_string(&self.dispatches_dir)
                ),
            )
        })?;

        let mut review_ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::DispatchWriteFailed,
                    format!(
                        "Could not read a review dispatch directory under {}: {error}",
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
                            "Could not inspect a review dispatch history entry under {}: {error}",
                            path_to_string(&self.dispatches_dir)
                        ),
                    )
                })?
                .is_dir()
            {
                continue;
            }

            let review_id = entry.file_name().to_string_lossy().into_owned();
            let validated_review_id = match validate_single_normal_path_component(
                &review_id,
                "Review id",
                ErrorCode::InvalidPathComponent,
            ) {
                Ok(review_id) => review_id,
                Err(error) => {
                    eprintln!(
                        "Skipping invalid review dispatch history directory {}: {}",
                        path_to_string(&entry.path()),
                        error
                    );
                    continue;
                }
            };

            review_ids.push(validated_review_id);
        }

        review_ids.sort();
        Ok(review_ids)
    }

    pub fn get_dispatch(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<Option<ReviewRunRecord>, TrackError> {
        let dispatch_record_path = self.dispatch_record_path(review_id, dispatch_id)?;
        if !dispatch_record_path.exists() {
            return Ok(None);
        }

        let record = self.read_dispatch_record_path(&dispatch_record_path)?;

        Ok(Some(record))
    }

    pub fn delete_dispatch_history_for_review(&self, review_id: &str) -> Result<(), TrackError> {
        let review_dispatch_directory = self.dispatch_directory_for_review(review_id)?;
        if !review_dispatch_directory.exists() {
            return Ok(());
        }

        fs::remove_dir_all(&review_dispatch_directory).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not remove the review dispatch history at {}: {error}",
                    path_to_string(&review_dispatch_directory)
                ),
            )
        })
    }

    fn dispatch_directory_for_review(&self, review_id: &str) -> Result<PathBuf, TrackError> {
        let review_id = validate_single_normal_path_component(
            review_id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        Ok(self.dispatches_dir.join(review_id))
    }

    fn dispatch_record_path(
        &self,
        review_id: &str,
        dispatch_id: &str,
    ) -> Result<PathBuf, TrackError> {
        let dispatch_id = validate_single_normal_path_component(
            dispatch_id,
            "Dispatch id",
            ErrorCode::InvalidPathComponent,
        )?;

        Ok(self
            .dispatch_directory_for_review(review_id)?
            .join(format!("{dispatch_id}.json")))
    }

    fn read_dispatch_record_path(&self, path: &PathBuf) -> Result<ReviewRunRecord, TrackError> {
        let raw_record = fs::read_to_string(path).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Could not read the review run record at {}: {error}",
                    path_to_string(path)
                ),
            )
        })?;

        serde_json::from_str::<ReviewRunRecord>(&raw_record).map_err(|error| {
            TrackError::new(
                ErrorCode::DispatchWriteFailed,
                format!(
                    "Review run record at {} is not valid JSON: {error}",
                    path_to_string(path)
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::ReviewDispatchRepository;
    use crate::time_utils::now_utc;
    use crate::types::{DispatchStatus, ReviewRecord, ReviewRunRecord};

    fn sample_review() -> ReviewRecord {
        let created_at = now_utc();
        ReviewRecord {
            id: "20260326-120000-review-pr-42".to_owned(),
            pull_request_url: "https://github.com/acme/project-a/pull/42".to_owned(),
            pull_request_number: 42,
            pull_request_title: "Fix queue layout".to_owned(),
            repository_full_name: "acme/project-a".to_owned(),
            repo_url: "https://github.com/acme/project-a".to_owned(),
            git_url: "git@github.com:acme/project-a.git".to_owned(),
            base_branch: "main".to_owned(),
            workspace_key: "project-a".to_owned(),
            project: Some("project-a".to_owned()),
            main_user: "octocat".to_owned(),
            default_review_prompt: Some("Look for risky behavior changes.".to_owned()),
            extra_instructions: Some("Pay attention to queue rendering.".to_owned()),
            created_at,
            updated_at: created_at,
        }
    }

    fn sample_review_run(review: &ReviewRecord, dispatch_id: &str) -> ReviewRunRecord {
        let created_at = now_utc();
        ReviewRunRecord {
            dispatch_id: dispatch_id.to_owned(),
            review_id: review.id.clone(),
            pull_request_url: review.pull_request_url.clone(),
            repository_full_name: review.repository_full_name.clone(),
            workspace_key: review.workspace_key.clone(),
            status: DispatchStatus::Succeeded,
            created_at,
            updated_at: created_at,
            finished_at: Some(created_at),
            remote_host: "127.0.0.1".to_owned(),
            branch_name: Some(format!("track-review/{dispatch_id}")),
            worktree_path: Some(format!(
                "~/workspace/{}/review-worktrees/{dispatch_id}",
                review.workspace_key
            )),
            summary: Some("Submitted a GitHub review.".to_owned()),
            review_submitted: true,
            github_review_id: Some("1001".to_owned()),
            github_review_url: Some(
                "https://github.com/acme/project-a/pull/42#pullrequestreview-1001".to_owned(),
            ),
            notes: None,
            error_message: None,
        }
    }

    #[test]
    fn skips_malformed_review_run_records_in_review_history() {
        let directory = TempDir::new().expect("tempdir should be created");
        let dispatches_dir = directory.path().join("reviews/.dispatches");
        let repository =
            ReviewDispatchRepository::new(Some(dispatches_dir.clone())).expect("repository");
        let review = sample_review();
        let run = sample_review_run(&review, "dispatch-1");
        repository
            .save_dispatch(&run)
            .expect("healthy review run should save");
        let broken_path = dispatches_dir.join(&review.id).join("broken-run.json");
        fs::create_dir_all(
            broken_path
                .parent()
                .expect("broken path should have a parent"),
        )
        .expect("review dispatch directory should exist");
        fs::write(&broken_path, "{not valid json").expect("broken review run should be written");

        let runs = repository
            .dispatches_for_review(&review.id)
            .expect("history should skip malformed review runs");

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].dispatch_id, run.dispatch_id);
    }

    #[test]
    fn skips_malformed_review_run_records_in_global_listing() {
        let directory = TempDir::new().expect("tempdir should be created");
        let dispatches_dir = directory.path().join("reviews/.dispatches");
        let repository =
            ReviewDispatchRepository::new(Some(dispatches_dir.clone())).expect("repository");
        let review = sample_review();
        let run = sample_review_run(&review, "dispatch-1");
        repository
            .save_dispatch(&run)
            .expect("healthy review run should save");
        let broken_directory = dispatches_dir.join("broken-review");
        fs::create_dir_all(&broken_directory).expect("broken review dir should exist");
        fs::write(
            broken_directory.join("dispatch-bad.json"),
            "{not valid json",
        )
        .expect("broken review run should be written");

        let runs = repository
            .list_dispatches(None)
            .expect("global listing should skip malformed review runs");

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].dispatch_id, run.dispatch_id);
    }

    #[test]
    fn loads_legacy_review_posted_field_for_existing_history() {
        let directory = TempDir::new().expect("tempdir should be created");
        let dispatches_dir = directory.path().join("reviews/.dispatches");
        let repository =
            ReviewDispatchRepository::new(Some(dispatches_dir.clone())).expect("repository");
        let review = sample_review();
        let review_directory = dispatches_dir.join(&review.id);
        fs::create_dir_all(&review_directory).expect("review dispatch dir should exist");
        fs::write(
            review_directory.join("dispatch-legacy.json"),
            serde_json::json!({
                "dispatchId": "dispatch-legacy",
                "reviewId": review.id,
                "pullRequestUrl": review.pull_request_url,
                "repositoryFullName": review.repository_full_name,
                "workspaceKey": review.workspace_key,
                "status": "succeeded",
                "createdAt": "2026-03-26T12:05:00.000Z",
                "updatedAt": "2026-03-26T12:06:00.000Z",
                "finishedAt": "2026-03-26T12:06:00.000Z",
                "remoteHost": "127.0.0.1",
                "branchName": "track-review/dispatch-legacy",
                "worktreePath": "~/workspace/project-a/review-worktrees/dispatch-legacy",
                "summary": "Legacy review run",
                "reviewPosted": true,
                "reviewBody": "@octocat requested me to review this PR."
            })
            .to_string(),
        )
        .expect("legacy review run should be written");

        let run = repository
            .get_dispatch(&review.id, "dispatch-legacy")
            .expect("legacy review run should load")
            .expect("legacy review run should exist");

        assert!(run.review_submitted);
    }
}
