use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::database::DatabaseContext;
use crate::errors::{ErrorCode, TrackError};
use crate::path_component::validate_single_normal_path_component;
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

#[derive(Debug, Clone)]
pub struct ProjectRepository {
    database: DatabaseContext,
}

impl ProjectRepository {
    pub fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize()?;

        Ok(Self { database })
    }

    pub fn ensure_project(&self, project: &ProjectInfo) -> Result<ProjectRecord, TrackError> {
        let metadata = build_default_metadata(project);
        self.upsert_project_by_name(&project.canonical_name, metadata, project.aliases.clone())
    }

    pub(crate) fn database_context(&self) -> DatabaseContext {
        self.database.clone()
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
        self.database.run(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    r#"
                    SELECT canonical_name, repo_url, git_url, base_branch, description
                    FROM projects
                    ORDER BY canonical_name ASC
                    "#,
                )
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::ProjectWriteFailed,
                        format!("Could not load projects from SQLite: {error}"),
                    )
                })?;

                let mut records = Vec::with_capacity(rows.len());
                for row in rows {
                    let canonical_name = row.get::<String, _>("canonical_name");
                    records.push(ProjectRecord {
                        aliases: load_aliases(connection, &canonical_name).await?,
                        metadata: ProjectMetadata {
                            repo_url: row.get::<String, _>("repo_url"),
                            git_url: row.get::<String, _>("git_url"),
                            base_branch: row.get::<String, _>("base_branch"),
                            description: row.get::<Option<String>, _>("description"),
                        },
                        canonical_name,
                    });
                }

                Ok(records)
            })
        })
    }

    pub fn get_project_by_name(&self, canonical_name: &str) -> Result<ProjectRecord, TrackError> {
        let canonical_name = validate_single_normal_path_component(
            canonical_name,
            "Project canonical name",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.run(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    r#"
                    SELECT canonical_name, repo_url, git_url, base_branch, description
                    FROM projects
                    WHERE canonical_name = ?1
                    "#,
                )
                .bind(&canonical_name)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::ProjectWriteFailed,
                        format!("Could not load project {canonical_name} from SQLite: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    TrackError::new(
                        ErrorCode::ProjectNotFound,
                        format!("Project {canonical_name} was not found."),
                    )
                })?;

                Ok(ProjectRecord {
                    aliases: load_aliases(connection, &canonical_name).await?,
                    metadata: ProjectMetadata {
                        repo_url: row.get::<String, _>("repo_url"),
                        git_url: row.get::<String, _>("git_url"),
                        base_branch: row.get::<String, _>("base_branch"),
                        description: row.get::<Option<String>, _>("description"),
                    },
                    canonical_name,
                })
            })
        })
    }

    pub fn update_project_by_name(
        &self,
        canonical_name: &str,
        metadata: ProjectMetadata,
    ) -> Result<ProjectRecord, TrackError> {
        let existing = self.get_project_by_name(canonical_name)?;
        self.upsert_project_by_name(&existing.canonical_name, metadata, existing.aliases)
    }

    pub fn upsert_project(&self, input: ProjectUpsertInput) -> Result<ProjectRecord, TrackError> {
        let (canonical_name, aliases, metadata) = input.validate()?;
        self.upsert_project_by_name(&canonical_name, metadata, aliases)
    }

    pub fn upsert_project_by_name(
        &self,
        canonical_name: &str,
        metadata: ProjectMetadata,
        aliases: Vec<String>,
    ) -> Result<ProjectRecord, TrackError> {
        let canonical_name = validate_single_normal_path_component(
            canonical_name,
            "Project canonical name",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database.transaction(move |connection| {
            Box::pin(async move {
                // Project registration is intentionally additive by default so
                // a routine re-registration cannot silently discard aliases
                // that were migrated from legacy state or added earlier.
                let mut merged_aliases = load_aliases(connection, &canonical_name).await?;
                merged_aliases.extend(aliases);
                merged_aliases.retain(|alias| alias != &canonical_name);
                merged_aliases.sort();
                merged_aliases.dedup();

                // Alias registration is part of the same logical write as the
                // project metadata update. We therefore reject conflicts before
                // mutating anything so callers never observe a half-applied
                // registration when another project already owns an alias.
                ensure_aliases_are_available(connection, &canonical_name, &merged_aliases).await?;

                sqlx::query(
                    r#"
                    INSERT INTO projects (canonical_name, repo_url, git_url, base_branch, description)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    ON CONFLICT(canonical_name) DO UPDATE SET
                        repo_url = excluded.repo_url,
                        git_url = excluded.git_url,
                        base_branch = excluded.base_branch,
                        description = excluded.description
                    "#,
                )
                .bind(&canonical_name)
                .bind(&metadata.repo_url)
                .bind(&metadata.git_url)
                .bind(&metadata.base_branch)
                .bind(metadata.description.as_deref())
                .execute(&mut *connection)
                .await
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::ProjectWriteFailed,
                        format!("Could not save project {canonical_name}: {error}"),
                    )
                })?;

                for alias in &merged_aliases {
                    sqlx::query(
                        r#"
                        INSERT INTO project_aliases (canonical_name, alias)
                        VALUES (?1, ?2)
                        ON CONFLICT(canonical_name, alias) DO NOTHING
                        "#,
                    )
                    .bind(&canonical_name)
                    .bind(alias)
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| {
                        TrackError::new(
                            ErrorCode::ProjectWriteFailed,
                            format!(
                                "Could not save the alias {alias} for project {canonical_name}: {error}"
                            ),
                        )
                    })?;
                }

                Ok(ProjectRecord {
                    canonical_name,
                    aliases: merged_aliases,
                    metadata,
                })
            })
        })
    }
}

pub fn infer_project_metadata(project: &ProjectInfo) -> ProjectMetadata {
    build_default_metadata(project)
}

async fn load_aliases(
    connection: &mut sqlx::SqliteConnection,
    canonical_name: &str,
) -> Result<Vec<String>, TrackError> {
    let rows = sqlx::query(
        r#"
        SELECT alias
        FROM project_aliases
        WHERE canonical_name = ?1
        ORDER BY alias ASC
        "#,
    )
    .bind(canonical_name)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| {
        TrackError::new(
            ErrorCode::ProjectWriteFailed,
            format!("Could not load project aliases for {canonical_name}: {error}"),
        )
    })?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<String, _>("alias"))
        .collect())
}

async fn ensure_aliases_are_available(
    connection: &mut sqlx::SqliteConnection,
    canonical_name: &str,
    aliases: &[String],
) -> Result<(), TrackError> {
    for alias in aliases {
        let row = sqlx::query(
            r#"
            SELECT canonical_name
            FROM project_aliases
            WHERE alias = ?1
            "#,
        )
        .bind(alias)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|error| {
            TrackError::new(
                ErrorCode::ProjectWriteFailed,
                format!("Could not verify whether alias {alias} is available: {error}"),
            )
        })?;

        if let Some(row) = row {
            let claimed_by = row.get::<String, _>("canonical_name");
            if claimed_by != canonical_name {
                return Err(TrackError::new(
                    ErrorCode::InvalidProjectMetadata,
                    format!("Project alias {alias} is already registered to project {claimed_by}."),
                ));
            }
        }
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{ProjectMetadata, ProjectRepository};
    use crate::errors::ErrorCode;

    fn metadata(description: &str) -> ProjectMetadata {
        ProjectMetadata {
            repo_url: "https://github.com/acme/project-a".to_owned(),
            git_url: "git@github.com:acme/project-a.git".to_owned(),
            base_branch: "main".to_owned(),
            description: Some(description.to_owned()),
        }
    }

    #[test]
    fn upsert_project_preserves_existing_aliases_when_no_new_aliases_are_provided() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["legacy-a".to_owned()])
            .expect("project should save");
        let project = repository
            .upsert_project_by_name("project-a", metadata("second"), Vec::new())
            .expect("project should update");

        assert_eq!(project.aliases, vec!["legacy-a".to_owned()]);
        assert_eq!(project.metadata.description.as_deref(), Some("second"));
    }

    #[test]
    fn upsert_project_unions_new_aliases_with_existing_aliases() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["legacy-a".to_owned()])
            .expect("project should save");
        let project = repository
            .upsert_project_by_name(
                "project-a",
                metadata("second"),
                vec!["new-a".to_owned(), "legacy-a".to_owned()],
            )
            .expect("project should update");

        assert_eq!(
            project.aliases,
            vec!["legacy-a".to_owned(), "new-a".to_owned()]
        );
    }

    #[test]
    fn upsert_project_rejects_conflicting_alias_without_partial_writes() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["shared".to_owned()])
            .expect("project a should save");
        repository
            .upsert_project_by_name("project-b", metadata("before"), Vec::new())
            .expect("project b should save");

        let error = repository
            .upsert_project_by_name("project-b", metadata("after"), vec!["shared".to_owned()])
            .expect_err("conflicting alias should fail");
        assert_eq!(error.code, ErrorCode::InvalidProjectMetadata);

        let project_b = repository
            .get_project_by_name("project-b")
            .expect("project b should still load");
        assert!(project_b.aliases.is_empty());
        assert_eq!(project_b.metadata.description.as_deref(), Some("before"));
    }
}
