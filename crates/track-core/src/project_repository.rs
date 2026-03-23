use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, TrackError};
use crate::paths::{get_data_dir, path_to_string};
use crate::project_catalog::ProjectInfo;

const PROJECT_METADATA_FILE_NAME: &str = "PROJECT.md";
const DEFAULT_BASE_BRANCH: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    #[serde(rename = "repoUrl")]
    pub repo_url: String,
    #[serde(rename = "gitUrl")]
    pub git_url: String,
    #[serde(rename = "baseBranch")]
    pub base_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectRecord {
    #[serde(rename = "canonicalName")]
    pub canonical_name: String,
    #[serde(with = "path_string")]
    pub path: PathBuf,
    pub aliases: Vec<String>,
    pub metadata: ProjectMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProjectMetadataUpdateInput {
    #[serde(rename = "repoUrl")]
    pub repo_url: String,
    #[serde(rename = "gitUrl")]
    pub git_url: String,
    #[serde(rename = "baseBranch")]
    pub base_branch: String,
    pub description: Option<String>,
}

impl ProjectMetadataUpdateInput {
    pub fn validate(self) -> Result<ProjectMetadata, TrackError> {
        let repo_url = self.repo_url.trim().to_owned();
        let git_url = self.git_url.trim().to_owned();
        let base_branch = self.base_branch.trim().to_owned();
        let description = self
            .description
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        if repo_url.is_empty() || git_url.is_empty() || base_branch.is_empty() {
            return Err(TrackError::new(
                ErrorCode::InvalidProjectMetadata,
                "Project metadata requires repo URL, git URL, and base branch.",
            ));
        }

        Ok(ProjectMetadata {
            repo_url,
            git_url,
            base_branch,
            description,
        })
    }
}

#[derive(Debug, Serialize)]
struct ProjectMetadataFrontmatter {
    #[serde(rename = "repoUrl")]
    repo_url: String,
    #[serde(rename = "gitUrl")]
    git_url: String,
    #[serde(rename = "baseBranch")]
    base_branch: String,
}

#[derive(Debug, Deserialize)]
struct ParsedProjectMetadataFrontmatter {
    #[serde(rename = "repoUrl")]
    repo_url: Option<String>,
    #[serde(rename = "gitUrl")]
    git_url: Option<String>,
    #[serde(rename = "baseBranch")]
    base_branch: Option<String>,
}

pub struct ProjectRepository {
    data_dir: PathBuf,
}

impl ProjectRepository {
    pub fn new(data_dir: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            data_dir: match data_dir {
                Some(path) => path,
                None => get_data_dir()?,
            },
        })
    }

    // =============================================================================
    // Host-Side Project Initialization
    // =============================================================================
    //
    // The CLI is the only process that can reliably see the user's checked-out
    // repositories, especially when the API runs in Docker with only the track
    // data directory mounted. That makes the CLI the right place to seed
    // `PROJECT.md` from git metadata.
    pub fn ensure_project(&self, project: &ProjectInfo) -> Result<ProjectRecord, TrackError> {
        let project_directory = self.ensure_project_directory(&project.canonical_name)?;
        let metadata_path = self.metadata_file_path(&project.canonical_name);

        let metadata = if metadata_path.exists() {
            self.load_existing_metadata_or_blank(&metadata_path)
        } else {
            let metadata = build_default_metadata(project);
            self.write_metadata_file(&metadata_path, &metadata)?;
            metadata
        };

        Ok(ProjectRecord {
            canonical_name: project.canonical_name.clone(),
            path: project_directory,
            aliases: project.aliases.clone(),
            metadata,
        })
    }

    // =============================================================================
    // Persisted Project Listing
    // =============================================================================
    //
    // The API and frontend work from the persisted track state only. They do
    // not rediscover repositories from the host filesystem because that breaks
    // down once the API is containerized. Listing projects therefore means
    // scanning the track data directory and treating each project folder as the
    // source of truth.
    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
        if !self.data_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.data_dir).map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!(
                    "Could not read the project directory at {}: {error}",
                    path_to_string(&self.data_dir)
                ),
            )
        })?;

        let mut records = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                TrackError::new(
                    ErrorCode::ProjectWriteFailed,
                    format!(
                        "Could not read a project entry under {}: {error}",
                        path_to_string(&self.data_dir)
                    ),
                )
            })?;

            let project_directory = entry.path();
            if !project_directory.is_dir() {
                continue;
            }

            let Some(canonical_name) = project_directory
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
            else {
                continue;
            };

            if canonical_name.starts_with('.') {
                continue;
            }

            let metadata_path = project_directory.join(PROJECT_METADATA_FILE_NAME);
            let metadata = if metadata_path.exists() {
                self.load_existing_metadata_or_blank(&metadata_path)
            } else {
                blank_project_metadata()
            };

            records.push(ProjectRecord {
                canonical_name,
                path: project_directory,
                aliases: Vec::new(),
                metadata,
            });
        }

        records.sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
        Ok(records)
    }

    pub fn get_project_by_name(&self, canonical_name: &str) -> Result<ProjectRecord, TrackError> {
        let project_directory = self.project_directory(canonical_name);
        if !project_directory.is_dir() {
            return Err(TrackError::new(
                ErrorCode::ProjectNotFound,
                format!("Project {canonical_name} was not found."),
            ));
        }

        let metadata_path = self.metadata_file_path(canonical_name);
        let metadata = if metadata_path.exists() {
            self.load_existing_metadata_or_blank(&metadata_path)
        } else {
            blank_project_metadata()
        };

        Ok(ProjectRecord {
            canonical_name: canonical_name.to_owned(),
            path: project_directory,
            aliases: Vec::new(),
            metadata,
        })
    }

    pub fn update_project_by_name(
        &self,
        canonical_name: &str,
        metadata: ProjectMetadata,
    ) -> Result<ProjectRecord, TrackError> {
        let project_directory = self.project_directory(canonical_name);
        if !project_directory.is_dir() {
            return Err(TrackError::new(
                ErrorCode::ProjectNotFound,
                format!("Project {canonical_name} was not found."),
            ));
        }

        self.write_metadata_file(&self.metadata_file_path(canonical_name), &metadata)?;

        Ok(ProjectRecord {
            canonical_name: canonical_name.to_owned(),
            path: project_directory,
            aliases: Vec::new(),
            metadata,
        })
    }

    fn ensure_project_directory(&self, canonical_name: &str) -> Result<PathBuf, TrackError> {
        let directory_path = self.project_directory(canonical_name);
        fs::create_dir_all(&directory_path).map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!(
                    "Could not create the project directory at {}: {error}",
                    path_to_string(&directory_path)
                ),
            )
        })?;

        Ok(directory_path)
    }

    fn project_directory(&self, canonical_name: &str) -> PathBuf {
        self.data_dir.join(canonical_name)
    }

    fn metadata_file_path(&self, canonical_name: &str) -> PathBuf {
        self.project_directory(canonical_name)
            .join(PROJECT_METADATA_FILE_NAME)
    }

    // =============================================================================
    // Metadata Read Strategy
    // =============================================================================
    //
    // Project metadata is intentionally hand-editable, so we keep task capture
    // and the project list resilient when a user leaves `PROJECT.md` in an
    // unfinished or malformed state. The persisted project directory is the
    // important durable signal; malformed metadata falls back to blank editable
    // fields instead of making the project disappear entirely.
    fn load_existing_metadata_or_blank(&self, metadata_path: &Path) -> ProjectMetadata {
        match self.read_metadata_file(metadata_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "Using blank project metadata for {}: {}",
                    path_to_string(metadata_path),
                    error
                );
                blank_project_metadata()
            }
        }
    }

    fn read_metadata_file(&self, file_path: &Path) -> Result<ProjectMetadata, TrackError> {
        let raw_file = fs::read_to_string(file_path).map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!(
                    "Could not read the project metadata file at {}: {error}",
                    path_to_string(file_path)
                ),
            )
        })?;

        let (frontmatter, body) = split_frontmatter(&raw_file)?;
        let parsed_frontmatter = serde_yaml::from_str::<ParsedProjectMetadataFrontmatter>(
            frontmatter,
        )
        .map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidProjectMetadata,
                format!(
                    "Project metadata at {} has invalid YAML frontmatter: {error}",
                    path_to_string(file_path)
                ),
            )
        })?;

        let repo_url = required_metadata_field(parsed_frontmatter.repo_url, "repoUrl", file_path)?;
        let git_url = required_metadata_field(parsed_frontmatter.git_url, "gitUrl", file_path)?;
        let base_branch =
            required_metadata_field(parsed_frontmatter.base_branch, "baseBranch", file_path)?;
        let description = body.trim().to_owned();

        Ok(ProjectMetadata {
            repo_url,
            git_url,
            base_branch,
            description: if description.is_empty() {
                None
            } else {
                Some(description)
            },
        })
    }

    fn write_metadata_file(
        &self,
        file_path: &Path,
        metadata: &ProjectMetadata,
    ) -> Result<(), TrackError> {
        let yaml = serde_yaml::to_string(&ProjectMetadataFrontmatter {
            repo_url: metadata.repo_url.clone(),
            git_url: metadata.git_url.clone(),
            base_branch: metadata.base_branch.clone(),
        })
        .map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!("Could not serialize project metadata: {error}"),
            )
        })?;

        let mut serialized = format!("---\n{}---\n", yaml);
        if let Some(description) = metadata.description.as_deref().map(str::trim) {
            if !description.is_empty() {
                serialized.push('\n');
                serialized.push_str(description);
                serialized.push('\n');
            }
        }

        fs::write(file_path, serialized).map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!(
                    "Could not write the project metadata file at {}: {error}",
                    path_to_string(file_path)
                ),
            )
        })
    }
}

fn blank_project_metadata() -> ProjectMetadata {
    ProjectMetadata {
        repo_url: String::new(),
        git_url: String::new(),
        base_branch: DEFAULT_BASE_BRANCH.to_owned(),
        description: None,
    }
}

// =============================================================================
// Default Metadata Inference
// =============================================================================
//
// Project metadata lives alongside track data rather than inside the Git
// checkout. When the CLI sees a repository for the first time, it seeds
// `PROJECT.md` with the details users are most likely to want to edit later:
// browser-friendly repo URL, clone URL, and default branch.
//
// We intentionally parse `.git/config` directly instead of shelling out to the
// `git` binary. That keeps initialization deterministic, avoids another runtime
// dependency, and makes the behavior easy to cover in tests with tiny fixture
// repositories.
fn build_default_metadata(project: &ProjectInfo) -> ProjectMetadata {
    let fallback_file_url = format!("file://{}", path_to_string(&project.path));
    let git_url = read_origin_git_url(&project.path).unwrap_or_else(|| fallback_file_url.clone());
    let repo_url = if git_url == fallback_file_url {
        fallback_file_url
    } else {
        derive_repo_url(&git_url)
    };

    ProjectMetadata {
        repo_url,
        git_url,
        base_branch: DEFAULT_BASE_BRANCH.to_owned(),
        description: None,
    }
}

fn read_origin_git_url(project_path: &Path) -> Option<String> {
    let git_directory = resolve_git_directory(project_path)?;
    let config = fs::read_to_string(git_directory.join("config")).ok()?;

    let mut in_origin_section = false;
    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_origin_section = line == "[remote \"origin\"]";
            continue;
        }

        if !in_origin_section {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "url" {
            continue;
        }

        let value = value.trim();
        if value.is_empty() {
            return None;
        }

        return Some(value.to_owned());
    }

    None
}

fn resolve_git_directory(project_path: &Path) -> Option<PathBuf> {
    let git_marker = project_path.join(".git");
    if git_marker.is_dir() {
        return Some(git_marker);
    }

    if !git_marker.is_file() {
        return None;
    }

    let contents = fs::read_to_string(git_marker).ok()?;
    let raw_git_dir = contents.trim().strip_prefix("gitdir:")?.trim();
    let git_dir = PathBuf::from(raw_git_dir);

    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(project_path.join(git_dir))
    }
}

fn derive_repo_url(git_url: &str) -> String {
    let git_url = git_url.trim();
    if git_url.is_empty() {
        return String::new();
    }

    if let Some(remainder) = git_url.strip_prefix("git@") {
        let Some((host, path)) = remainder.split_once(':') else {
            return trim_git_suffix(git_url).to_owned();
        };
        return format!("https://{host}/{}", trim_git_suffix(path));
    }

    if let Some(remainder) = git_url.strip_prefix("ssh://") {
        let remainder = remainder
            .split_once('@')
            .map(|(_, value)| value)
            .unwrap_or(remainder);

        let Some((host, path)) = remainder.split_once('/') else {
            return trim_git_suffix(git_url).to_owned();
        };
        return format!("https://{host}/{}", trim_git_suffix(path));
    }

    trim_git_suffix(git_url).to_owned()
}

fn trim_git_suffix(value: &str) -> &str {
    value.trim_end_matches(".git").trim_end_matches('/')
}

fn required_metadata_field(
    value: Option<String>,
    field_name: &str,
    file_path: &Path,
) -> Result<String, TrackError> {
    let value = value.unwrap_or_default().trim().to_owned();
    if value.is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            format!(
                "Project metadata at {} is missing a required `{field_name}` field.",
                path_to_string(file_path)
            ),
        ));
    }

    Ok(value)
}

fn split_frontmatter(raw_file: &str) -> Result<(&str, &str), TrackError> {
    let Some(after_start) = consume_frontmatter_delimiter(raw_file, 0) else {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            "Project metadata file must start with YAML frontmatter.",
        ));
    };

    let Some(end_start) = find_frontmatter_end(raw_file, after_start) else {
        return Err(TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            "Project metadata file is missing the closing YAML frontmatter delimiter.",
        ));
    };

    let frontmatter = &raw_file[after_start..end_start];
    let body_start = consume_frontmatter_delimiter(raw_file, end_start).ok_or_else(|| {
        TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            "Project metadata file is missing the closing YAML frontmatter delimiter.",
        )
    })?;

    Ok((frontmatter, &raw_file[body_start..]))
}

fn consume_frontmatter_delimiter(raw_file: &str, offset: usize) -> Option<usize> {
    let after = raw_file.get(offset..)?;
    if let Some(remainder) = after.strip_prefix("---\r\n") {
        return Some(raw_file.len() - remainder.len());
    }

    if let Some(remainder) = after.strip_prefix("---\n") {
        return Some(raw_file.len() - remainder.len());
    }

    None
}

fn find_frontmatter_end(raw_file: &str, start: usize) -> Option<usize> {
    let after = raw_file.get(start..)?;
    after
        .find("\n---\n")
        .map(|index| start + index + 1)
        .or_else(|| after.find("\n---\r\n").map(|index| start + index + 1))
        .or_else(|| after.find("\r\n---\n").map(|index| start + index + 2))
        .or_else(|| after.find("\r\n---\r\n").map(|index| start + index + 2))
}

mod path_string {
    use std::path::PathBuf;

    use serde::Serializer;

    pub fn serialize<S>(path: &PathBuf, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&path.to_string_lossy())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{
        derive_repo_url, read_origin_git_url, ProjectMetadataUpdateInput, ProjectRepository,
        DEFAULT_BASE_BRANCH,
    };
    use crate::{errors::ErrorCode, paths::path_to_string, project_catalog::ProjectInfo};

    #[test]
    fn ensure_project_creates_default_metadata_from_origin_remote() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");
        let project_path = directory.path().join("workspace/project-x");
        fs::create_dir_all(project_path.join(".git")).expect("git directory should exist");
        fs::write(
            project_path.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:acme/project-x.git\n",
        )
        .expect("git config should be written");

        let record = repository
            .ensure_project(&ProjectInfo {
                canonical_name: "project-x".to_owned(),
                path: project_path.clone(),
                aliases: vec!["proj-x".to_owned()],
            })
            .expect("project should be initialized");

        assert_eq!(record.metadata.git_url, "git@github.com:acme/project-x.git");
        assert_eq!(
            record.metadata.repo_url,
            "https://github.com/acme/project-x"
        );
        assert_eq!(record.metadata.base_branch, "main");
        assert_eq!(record.metadata.description, None);
        assert_eq!(
            record.path,
            directory.path().join("issues").join("project-x")
        );
        assert!(directory
            .path()
            .join("issues/project-x/PROJECT.md")
            .exists());
    }

    #[test]
    fn list_projects_includes_existing_project_directories_without_metadata() {
        let directory = TempDir::new().expect("tempdir should be created");
        let issues_path = directory.path().join("issues");
        fs::create_dir_all(issues_path.join("project-a/open"))
            .expect("project directory should exist");
        fs::create_dir_all(issues_path.join("project-b")).expect("project directory should exist");

        let repository =
            ProjectRepository::new(Some(issues_path)).expect("repository should resolve");
        let records = repository.list_projects().expect("projects should list");

        assert_eq!(
            records
                .iter()
                .map(|record| record.canonical_name.as_str())
                .collect::<Vec<_>>(),
            vec!["project-a", "project-b"]
        );
        assert_eq!(records[0].metadata.repo_url, "");
        assert_eq!(records[0].metadata.git_url, "");
        assert_eq!(records[0].metadata.base_branch, DEFAULT_BASE_BRANCH);
        assert_eq!(records[0].metadata.description, None);
    }

    #[test]
    fn update_project_rewrites_the_metadata_markdown_file() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");
        repository
            .ensure_project(&ProjectInfo {
                canonical_name: "project-x".to_owned(),
                path: directory.path().join("workspace/project-x"),
                aliases: vec![],
            })
            .expect("project should initialize");

        let updated = repository
            .update_project_by_name(
                "project-x",
                ProjectMetadataUpdateInput {
                    repo_url: "https://github.com/acme/project-x".to_owned(),
                    git_url: "git@github.com:acme/project-x.git".to_owned(),
                    base_branch: "develop".to_owned(),
                    description: Some("Main coordination repo for the release work.".to_owned()),
                }
                .validate()
                .expect("metadata should validate"),
            )
            .expect("project should update");

        assert_eq!(updated.metadata.base_branch, "develop");
        let raw_file = fs::read_to_string(directory.path().join("issues/project-x/PROJECT.md"))
            .expect("metadata file should be readable");
        assert!(raw_file.contains("baseBranch: develop"));
        assert!(raw_file.contains("Main coordination repo for the release work."));
    }

    #[test]
    fn validates_required_project_metadata_fields() {
        let error = ProjectMetadataUpdateInput {
            repo_url: " ".to_owned(),
            git_url: "git@github.com:acme/project-x.git".to_owned(),
            base_branch: "main".to_owned(),
            description: None,
        }
        .validate()
        .expect_err("metadata should reject empty repo url");

        assert_eq!(error.code, ErrorCode::InvalidProjectMetadata);
    }

    #[test]
    fn derives_repo_urls_from_common_git_remote_shapes() {
        assert_eq!(
            derive_repo_url("git@github.com:acme/project-x.git"),
            "https://github.com/acme/project-x"
        );
        assert_eq!(
            derive_repo_url("ssh://git@gitlab.com/acme/project-x.git"),
            "https://gitlab.com/acme/project-x"
        );
        assert_eq!(
            derive_repo_url("https://github.com/acme/project-x.git"),
            "https://github.com/acme/project-x"
        );
    }

    #[test]
    fn reads_origin_remote_from_git_config() {
        let directory = TempDir::new().expect("tempdir should be created");
        let project_path = directory.path().join("workspace/project-x");
        fs::create_dir_all(project_path.join(".git")).expect("git directory should exist");
        fs::write(
            project_path.join(".git/config"),
            "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://github.com/acme/project-x.git\n",
        )
        .expect("git config should be written");

        assert_eq!(
            read_origin_git_url(&project_path),
            Some("https://github.com/acme/project-x.git".to_owned())
        );
    }

    #[test]
    fn falls_back_to_file_urls_when_origin_remote_is_missing() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("issues")))
            .expect("repository should resolve");
        let project_path = directory.path().join("workspace/project-x");
        fs::create_dir_all(project_path.join(".git")).expect("git directory should exist");
        fs::write(project_path.join(".git/config"), "[core]\n\tbare = false\n")
            .expect("git config should be written");

        let record = repository
            .ensure_project(&ProjectInfo {
                canonical_name: "project-x".to_owned(),
                path: project_path.clone(),
                aliases: vec![],
            })
            .expect("project should initialize");

        let expected_file_url = format!("file://{}", path_to_string(&project_path));
        assert_eq!(record.metadata.repo_url, expected_file_url);
        assert_eq!(record.metadata.git_url, expected_file_url);
    }
}
