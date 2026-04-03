use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;

use crate::project_catalog::ProjectInfo;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    #[serde(rename = "canonicalName")]
    pub canonical_name: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProjectUpsertInput {
    #[serde(rename = "canonicalName")]
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(flatten)]
    pub metadata: ProjectMetadataUpdateInput,
}

impl ProjectUpsertInput {
    pub fn validate(self) -> Result<(String, Vec<String>, ProjectMetadata), TrackError> {
        let canonical_name = validate_single_normal_path_component(
            &self.canonical_name,
            "Project canonical name",
            ErrorCode::InvalidPathComponent,
        )?;
        let mut aliases = self
            .aliases
            .into_iter()
            .map(|alias| {
                validate_single_normal_path_component(
                    &alias,
                    "Project alias",
                    ErrorCode::InvalidPathComponent,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        aliases.sort();
        aliases.dedup();

        Ok((canonical_name, aliases, self.metadata.validate()?))
    }
}

pub fn infer_project_metadata(project: &ProjectInfo) -> ProjectMetadata {
    build_default_metadata(project)
}

fn build_default_metadata(project: &ProjectInfo) -> ProjectMetadata {
    let fallback_file_url = format!("file://{}", project.path.to_string_lossy());
    let git_url = read_origin_git_url(&project.path).unwrap_or_else(|| fallback_file_url.clone());
    let repo_url = if git_url == fallback_file_url {
        fallback_file_url
    } else {
        derive_repo_url(&git_url)
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

pub fn derive_repo_url(git_url: &str) -> String {
    let git_url = git_url.trim();

    if let Some(path) = git_url.strip_prefix("git@") {
        if let Some((host, repo_path)) = path.split_once(':') {
            return format!("https://{host}/{}", repo_path.trim_end_matches(".git"));
        }
    }

    git_url
        .trim_end_matches(".git")
        .replace("ssh://git@", "https://")
        .replace("ssh://", "https://")
}
