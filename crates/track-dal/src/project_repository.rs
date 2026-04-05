use std::path::PathBuf;

use sqlx::Row;
use track_projects::project_catalog::ProjectInfo;
use track_projects::project_metadata::{
    infer_project_metadata, ProjectMetadata, ProjectRecord, ProjectUpsertInput,
};
use track_types::errors::{ErrorCode, TrackError};
use track_types::path_component::validate_single_normal_path_component;

use crate::database::DatabaseContext;

#[derive(Debug, Clone)]
pub struct ProjectRepository {
    database: DatabaseContext,
}

impl ProjectRepository {
    pub async fn new(database_path: Option<PathBuf>) -> Result<Self, TrackError> {
        let database = DatabaseContext::new(database_path)?;
        database.initialize().await?;

        Ok(Self { database })
    }

    pub async fn ensure_project(&self, project: &ProjectInfo) -> Result<ProjectRecord, TrackError> {
        let metadata = infer_project_metadata(project);
        self.upsert_project_by_name(&project.canonical_name, metadata, project.aliases.clone())
            .await
    }

    pub fn database_context(&self) -> DatabaseContext {
        self.database.clone()
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
        self.database
            .run(move |connection| {
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
            .await
    }

    pub async fn get_project_by_name(
        &self,
        canonical_name: &str,
    ) -> Result<ProjectRecord, TrackError> {
        let canonical_name = validate_single_normal_path_component(
            canonical_name,
            "Project canonical name",
            ErrorCode::InvalidPathComponent,
        )?;

        self.database
            .run(move |connection| {
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
            .await
    }

    pub async fn update_project_by_name(
        &self,
        canonical_name: &str,
        metadata: ProjectMetadata,
    ) -> Result<ProjectRecord, TrackError> {
        let existing = self.get_project_by_name(canonical_name).await?;
        self.upsert_project_by_name(&existing.canonical_name, metadata, existing.aliases)
            .await
    }

    pub async fn upsert_project(
        &self,
        input: ProjectUpsertInput,
    ) -> Result<ProjectRecord, TrackError> {
        let (canonical_name, aliases, metadata) = input.validate()?;
        self.upsert_project_by_name(&canonical_name, metadata, aliases)
            .await
    }

    pub async fn upsert_project_by_name(
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
        }).await
    }
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use track_types::errors::ErrorCode;

    use super::ProjectRepository;
    use track_projects::project_metadata::ProjectMetadata;

    fn metadata(description: &str) -> ProjectMetadata {
        ProjectMetadata {
            repo_url: "https://github.com/acme/project-a".to_owned(),
            git_url: "git@github.com:acme/project-a.git".to_owned(),
            base_branch: "main".to_owned(),
            description: Some(description.to_owned()),
        }
    }

    #[tokio::test]
    async fn upsert_project_preserves_existing_aliases_when_no_new_aliases_are_provided() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .await
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["legacy-a".to_owned()])
            .await
            .expect("project should save");
        let project = repository
            .upsert_project_by_name("project-a", metadata("second"), Vec::new())
            .await
            .expect("project should update");

        assert_eq!(project.aliases, vec!["legacy-a".to_owned()]);
        assert_eq!(project.metadata.description.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn upsert_project_unions_new_aliases_with_existing_aliases() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .await
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["legacy-a".to_owned()])
            .await
            .expect("project should save");
        let project = repository
            .upsert_project_by_name(
                "project-a",
                metadata("second"),
                vec!["new-a".to_owned(), "legacy-a".to_owned()],
            )
            .await
            .expect("project should update");

        assert_eq!(
            project.aliases,
            vec!["legacy-a".to_owned(), "new-a".to_owned()]
        );
    }

    #[tokio::test]
    async fn upsert_project_rejects_conflicting_alias_without_partial_writes() {
        let directory = TempDir::new().expect("tempdir should be created");
        let repository = ProjectRepository::new(Some(directory.path().join("track.sqlite")))
            .await
            .expect("project repository should resolve");

        repository
            .upsert_project_by_name("project-a", metadata("first"), vec!["shared".to_owned()])
            .await
            .expect("project a should save");
        repository
            .upsert_project_by_name("project-b", metadata("before"), Vec::new())
            .await
            .expect("project b should save");

        let error = repository
            .upsert_project_by_name("project-b", metadata("after"), vec!["shared".to_owned()])
            .await
            .expect_err("conflicting alias should fail");
        assert_eq!(error.code, ErrorCode::InvalidProjectMetadata);

        let project_b = repository
            .get_project_by_name("project-b")
            .await
            .expect("project b should still load");
        assert!(project_b.aliases.is_empty());
        assert_eq!(project_b.metadata.description.as_deref(), Some("before"));
    }
}
