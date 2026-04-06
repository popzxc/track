use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use track_types::errors::{ErrorCode, TrackError};
use track_types::ids::ProjectId;
use track_types::urls::{parse_url, Url};

use crate::project_catalog::ProjectInfo;

const DEFAULT_BASE_BRANCH: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub repo_url: Url,
    pub git_url: String,
    pub base_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub canonical_name: ProjectId,
    pub aliases: Vec<ProjectId>,
    pub metadata: ProjectMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadataUpdateInput {
    pub repo_url: String,
    pub git_url: String,
    pub base_branch: String,
    pub description: Option<String>,
}

impl ProjectMetadataUpdateInput {
    pub fn validate(self) -> Result<ProjectMetadata, TrackError> {
        let repo_url = self.repo_url.trim();
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
            repo_url: parse_url(
                repo_url,
                ErrorCode::InvalidProjectMetadata,
                format!("Project repo URL `{repo_url}` is not valid"),
            )?,
            git_url,
            base_branch,
            description,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpsertInput {
    pub canonical_name: ProjectId,
    #[serde(default)]
    pub aliases: Vec<ProjectId>,
    #[serde(flatten)]
    pub metadata: ProjectMetadataUpdateInput,
}

impl ProjectUpsertInput {
    pub fn validate(self) -> Result<(ProjectId, Vec<ProjectId>, ProjectMetadata), TrackError> {
        let canonical_name = self.canonical_name;
        let mut aliases = self.aliases.into_iter().collect::<Vec<_>>();
        aliases.sort();
        aliases.dedup();

        Ok((canonical_name, aliases, self.metadata.validate()?))
    }
}

pub fn infer_project_metadata(project: &ProjectInfo) -> ProjectMetadata {
    build_default_metadata(project)
}

fn build_default_metadata(project: &ProjectInfo) -> ProjectMetadata {
    let fallback_file_url =
        Url::from_file_path(&project.path).expect("discovered project paths should be absolute");
    let fallback_file_url_string = fallback_file_url.to_string();
    let git_url = read_origin_git_url(&project.path).unwrap_or_else(|| fallback_file_url_string);
    let repo_url = if git_url == fallback_file_url.as_str() {
        fallback_file_url
    } else {
        derive_repo_url(&git_url).unwrap_or(fallback_file_url)
    };
    let base_branch =
        infer_default_base_branch(&project.path).unwrap_or_else(|| DEFAULT_BASE_BRANCH.to_owned());

    ProjectMetadata {
        repo_url,
        git_url,
        base_branch,
        description: None,
    }
}

fn infer_default_base_branch(project_path: &Path) -> Option<String> {
    let git_common_directory = resolve_git_common_directory(project_path)?;

    read_symbolic_ref_branch(
        &git_common_directory.join("refs/remotes/origin/HEAD"),
        "refs/remotes/origin/",
    )
    .or_else(|| read_symbolic_ref_branch(&git_common_directory.join("HEAD"), "refs/heads/"))
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

    let gitdir_directive = fs::read_to_string(git_marker).ok()?;
    let relative_path = gitdir_directive
        .trim()
        .strip_prefix("gitdir:")?
        .trim()
        .to_owned();

    Some(project_path.join(relative_path))
}

fn resolve_git_common_directory(project_path: &Path) -> Option<PathBuf> {
    let git_directory = resolve_git_directory(project_path)?;
    let commondir_path = git_directory.join("commondir");
    if !commondir_path.is_file() {
        return Some(git_directory);
    }

    let relative = fs::read_to_string(commondir_path).ok()?;
    Some(git_directory.join(relative.trim()))
}

fn read_symbolic_ref_branch(reference_path: &Path, prefix: &str) -> Option<String> {
    let symbolic_ref = fs::read_to_string(reference_path).ok()?;
    symbolic_ref
        .trim()
        .strip_prefix("ref:")?
        .trim()
        .strip_prefix(prefix)
        .map(str::to_owned)
}

pub fn derive_repo_url(git_url: &str) -> Option<Url> {
    let git_url = git_url.trim();

    if let Some(path) = git_url.strip_prefix("git@") {
        if let Some((host, repo_path)) = path.split_once(':') {
            return Url::parse(&format!(
                "https://{host}/{}",
                repo_path.trim_end_matches(".git")
            ))
            .ok();
        }
    }

    let trimmed = git_url.trim_end_matches(".git");
    let https_candidate = trimmed
        .replace("ssh://git@", "https://")
        .replace("ssh://", "https://");

    Url::parse(&https_candidate).ok()
}

#[cfg(test)]
mod tests {
    use track_types::ids::ProjectId;
    use track_types::urls::Url;

    use crate::project_catalog::ProjectInfo;

    use super::{derive_repo_url, infer_project_metadata, ProjectMetadataUpdateInput};

    #[test]
    fn validates_project_metadata_urls() {
        let metadata = ProjectMetadataUpdateInput {
            repo_url: " https://github.com/acme/project-a ".to_owned(),
            git_url: "git@github.com:acme/project-a.git".to_owned(),
            base_branch: " main ".to_owned(),
            description: Some(" Release coordination ".to_owned()),
        }
        .validate()
        .expect("project metadata should validate");

        assert_eq!(
            metadata.repo_url.as_str(),
            "https://github.com/acme/project-a"
        );
        assert_eq!(metadata.base_branch, "main");
        assert_eq!(
            metadata.description.as_deref(),
            Some("Release coordination")
        );
    }

    #[test]
    fn derives_https_repo_url_from_git_ssh_remote() {
        let repo_url = derive_repo_url("git@github.com:acme/project-a.git")
            .expect("ssh git remote should derive a repo url");

        assert_eq!(repo_url.as_str(), "https://github.com/acme/project-a");
    }

    #[test]
    fn falls_back_to_a_file_url_when_git_metadata_is_missing() {
        let tempdir = tempfile::TempDir::new().expect("tempdir should be created");
        let project_path = tempdir.path().join("project-a");
        std::fs::create_dir_all(&project_path).expect("project path should exist");
        std::fs::create_dir_all(project_path.join(".git")).expect("git marker should exist");
        let project = ProjectInfo {
            canonical_name: ProjectId::new("project-a").unwrap(),
            path: project_path.clone(),
            aliases: Vec::new(),
        };

        let metadata = infer_project_metadata(&project);

        assert_eq!(
            metadata.repo_url,
            Url::from_file_path(project_path).expect("fixture path should become a file url")
        );
    }
}
