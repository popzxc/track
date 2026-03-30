use std::fs;
use std::path::PathBuf;

use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;
use crate::paths::{get_reviews_dir, path_to_string};
use crate::types::ReviewRecord;

pub struct ReviewRepository {
    reviews_dir: PathBuf,
}

impl ReviewRepository {
    pub fn new(reviews_dir: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            reviews_dir: match reviews_dir {
                Some(reviews_dir) => reviews_dir,
                None => get_reviews_dir()?,
            },
        })
    }

    pub fn reviews_dir(&self) -> &PathBuf {
        &self.reviews_dir
    }

    pub fn save_review(&self, review: &ReviewRecord) -> Result<(), TrackError> {
        fs::create_dir_all(&self.reviews_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not create the reviews directory at {}: {error}",
                    path_to_string(&self.reviews_dir)
                ),
            )
        })?;

        let serialized = serde_json::to_string_pretty(review).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!("Could not serialize the review record: {error}"),
            )
        })?;

        fs::write(self.review_path(&review.id)?, format!("{serialized}\n")).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not write the review record for {}: {error}",
                    review.id
                ),
            )
        })
    }

    pub fn list_reviews(&self) -> Result<Vec<ReviewRecord>, TrackError> {
        if !self.reviews_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.reviews_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not read the reviews directory at {}: {error}",
                    path_to_string(&self.reviews_dir)
                ),
            )
        })?;

        let mut reviews = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::TaskWriteFailed,
                    format!(
                        "Could not read a review entry under {}: {error}",
                        path_to_string(&self.reviews_dir)
                    ),
                )
            })?;

            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            match self.read_review_path(&path) {
                Ok(review) => reviews.push(review),
                Err(error) => {
                    eprintln!(
                        "Skipping malformed review record at {}: {}",
                        path_to_string(&path),
                        error
                    );
                }
            }
        }

        reviews.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(reviews)
    }

    pub fn get_review(&self, id: &str) -> Result<ReviewRecord, TrackError> {
        let path = self.review_path(id)?;
        if !path.exists() {
            return Err(TrackError::new(
                ErrorCode::TaskNotFound,
                format!("Review {id} was not found."),
            ));
        }

        self.read_review_path(&path)
    }

    pub fn delete_review(&self, id: &str) -> Result<(), TrackError> {
        let path = self.review_path(id)?;
        if !path.exists() {
            return Ok(());
        }

        fs::remove_file(&path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not delete the review record at {}: {error}",
                    path_to_string(&path)
                ),
            )
        })
    }

    fn review_path(&self, id: &str) -> Result<PathBuf, TrackError> {
        let id = validate_single_normal_path_component(
            id,
            "Review id",
            ErrorCode::InvalidPathComponent,
        )?;

        Ok(self.reviews_dir.join(format!("{id}.json")))
    }

    fn read_review_path(&self, path: &PathBuf) -> Result<ReviewRecord, TrackError> {
        let raw_review = fs::read_to_string(path).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Could not read the review record at {}: {error}",
                    path_to_string(path)
                ),
            )
        })?;

        serde_json::from_str::<ReviewRecord>(&raw_review).map_err(|error| {
            TrackError::new(
                ErrorCode::TaskWriteFailed,
                format!(
                    "Review record at {} is not valid JSON: {error}",
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

    use super::ReviewRepository;
    use crate::time_utils::now_utc;
    use crate::types::ReviewRecord;

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

    #[test]
    fn saves_and_lists_reviews_newest_first() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository =
            ReviewRepository::new(Some(directory.path().join("reviews"))).expect("repository");

        let review = sample_review();
        repository.save_review(&review).expect("review should save");

        let reviews = repository.list_reviews().expect("reviews should load");
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].id, review.id);
        assert_eq!(reviews[0].pull_request_url, review.pull_request_url);
        assert_eq!(reviews[0].main_user, review.main_user);
        assert_eq!(
            reviews[0].default_review_prompt,
            review.default_review_prompt
        );
    }

    #[test]
    fn lists_reviews_by_latest_update_time() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository =
            ReviewRepository::new(Some(directory.path().join("reviews"))).expect("repository");

        let older_review = sample_review();
        let newer_review = ReviewRecord {
            id: "20260326-120100-review-pr-43".to_owned(),
            updated_at: older_review.updated_at + time::Duration::minutes(5),
            ..sample_review()
        };

        repository
            .save_review(&older_review)
            .expect("older review should save");
        repository
            .save_review(&newer_review)
            .expect("newer review should save");

        let reviews = repository.list_reviews().expect("reviews should load");

        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].id, newer_review.id);
        assert_eq!(reviews[1].id, older_review.id);
    }

    #[test]
    fn skips_malformed_review_records_during_listing() {
        let directory = TempDir::new().expect("tempdir should be created");
        let reviews_dir = directory.path().join("reviews");
        let repository = ReviewRepository::new(Some(reviews_dir.clone())).expect("repository");

        let review = sample_review();
        repository.save_review(&review).expect("review should save");
        fs::write(reviews_dir.join("broken-review.json"), "{not valid json")
            .expect("broken review should be written");

        let reviews = repository
            .list_reviews()
            .expect("listing should skip malformed review records");

        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].id, review.id);
    }
}
