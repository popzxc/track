use serde::{Deserialize, Serialize};
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::ids::ProjectId;
use track_types::urls::{parse_url, Url};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMetadata {
    pub repo_url: Url,
    pub git_url: GitRemote,
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
            git_url: GitRemote::new(&git_url).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidProjectMetadata,
                    format!(
                        "Project git remote `{git_url}` is not valid: {}",
                        error.message()
                    ),
                )
            })?,
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

#[cfg(test)]
mod tests {
    use super::ProjectMetadataUpdateInput;

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
        assert_eq!(
            metadata.git_url.clone().into_remote_string(),
            "git@github.com:acme/project-a.git"
        );
        assert_eq!(metadata.base_branch, "main");
        assert_eq!(
            metadata.description.as_deref(),
            Some("Release coordination")
        );
    }
}
