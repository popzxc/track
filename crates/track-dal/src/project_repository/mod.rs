mod records;

use track_projects::project_metadata::{ProjectMetadata, ProjectRecord, ProjectUpsertInput};
use track_types::errors::{ErrorCode, TrackError};
use track_types::git_remote::GitRemote;
use track_types::ids::ProjectId;
use track_types::urls::parse_persisted_url;

use crate::database::{DatabaseContext, DatabaseResultExt};

#[derive(Debug, Clone, Copy)]
pub struct ProjectRepository<'a> {
    database: &'a DatabaseContext,
}

impl<'a> ProjectRepository<'a> {
    pub(crate) fn new(database: &'a DatabaseContext) -> Self {
        Self { database }
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, TrackError> {
        let mut connection = self.database.connect().await?;
        let rows = sqlx::query_as!(
            records::ProjectRow,
            r#"
            SELECT
                canonical_name AS "canonical_name!",
                repo_url AS "repo_url!",
                git_url AS "git_url!",
                base_branch AS "base_branch!",
                description AS "description?"
            FROM projects
            ORDER BY canonical_name ASC
            "#,
        )
        .fetch_all(&mut *connection)
        .await
        .database_error_with("Could not load projects from SQLite")?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let canonical_name = ProjectId::from_db(row.canonical_name);
            records.push(ProjectRecord {
                aliases: load_aliases(&mut connection, &canonical_name).await?,
                metadata: ProjectMetadata {
                    repo_url: parse_persisted_url(
                        row.repo_url,
                        "stored project repo URLs should be valid",
                    ),
                    git_url: GitRemote::from_db(row.git_url),
                    base_branch: row.base_branch,
                    description: row.description,
                },
                canonical_name,
            });
        }

        Ok(records)
    }

    pub async fn get_project_by_name(
        &self,
        canonical_name: &ProjectId,
    ) -> Result<ProjectRecord, TrackError> {
        let mut connection = self.database.connect().await?;
        let canonical_name_ref = canonical_name.as_str();
        let row = sqlx::query_as!(
            records::ProjectRow,
            r#"
            SELECT
                canonical_name AS "canonical_name!",
                repo_url AS "repo_url!",
                git_url AS "git_url!",
                base_branch AS "base_branch!",
                description AS "description?"
            FROM projects
            WHERE canonical_name = ?1
            "#,
            canonical_name_ref,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not load project {canonical_name} from SQLite"
        ))?
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::ProjectNotFound,
                format!("Project {canonical_name} was not found."),
            )
        })?;

        Ok(ProjectRecord {
            aliases: load_aliases(&mut connection, canonical_name).await?,
            metadata: ProjectMetadata {
                repo_url: parse_persisted_url(
                    row.repo_url,
                    "stored project repo URLs should be valid",
                ),
                git_url: GitRemote::from_db(row.git_url),
                base_branch: row.base_branch,
                description: row.description,
            },
            canonical_name: canonical_name.clone(),
        })
    }

    pub async fn update_project_by_name(
        &self,
        canonical_name: &ProjectId,
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
        canonical_name: &ProjectId,
        metadata: ProjectMetadata,
        aliases: Vec<ProjectId>,
    ) -> Result<ProjectRecord, TrackError> {
        let canonical_name = canonical_name.clone();

        let mut transaction = self.database.begin().await?;

        // Project registration is intentionally additive by default so a
        // routine re-registration cannot silently discard aliases that were
        // migrated from legacy state or added earlier.
        let mut merged_aliases = load_aliases(&mut transaction, &canonical_name).await?;
        merged_aliases.extend(aliases);
        merged_aliases.retain(|alias| alias != &canonical_name);
        merged_aliases.sort();
        merged_aliases.dedup();

        // Alias registration is part of the same logical write as the project
        // metadata update. We therefore reject conflicts before mutating
        // anything so callers never observe a half-applied registration when
        // another project already owns an alias.
        ensure_aliases_are_available(&mut transaction, &canonical_name, &merged_aliases).await?;
        let canonical_name_ref = canonical_name.as_str();
        let repo_url = metadata.repo_url.as_str();
        let git_url = metadata.git_url.clone().into_remote_string();
        let git_url_ref = git_url.as_str();
        let base_branch = metadata.base_branch.as_str();
        let description = metadata.description.as_deref();

        sqlx::query!(
            r#"
            INSERT INTO projects (canonical_name, repo_url, git_url, base_branch, description)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(canonical_name) DO UPDATE SET
                repo_url = excluded.repo_url,
                git_url = excluded.git_url,
                base_branch = excluded.base_branch,
                description = excluded.description
            "#,
            canonical_name_ref,
            repo_url,
            git_url_ref,
            base_branch,
            description,
        )
        .execute(&mut *transaction)
        .await
        .database_error_with(format!("Could not save project {canonical_name}"))?;

        for alias in &merged_aliases {
            let alias_name = alias.as_str();
            sqlx::query!(
                r#"
                INSERT INTO project_aliases (canonical_name, alias)
                VALUES (?1, ?2)
                ON CONFLICT(canonical_name, alias) DO NOTHING
                "#,
                canonical_name_ref,
                alias_name,
            )
            .execute(&mut *transaction)
            .await
            .database_error_with(format!(
                "Could not save the alias {alias} for project {canonical_name}"
            ))?;
        }

        transaction
            .commit()
            .await
            .database_error_with("Could not commit the project transaction")?;

        Ok(ProjectRecord {
            canonical_name,
            aliases: merged_aliases,
            metadata,
        })
    }
}

async fn load_aliases(
    connection: &mut sqlx::SqliteConnection,
    canonical_name: &ProjectId,
) -> Result<Vec<ProjectId>, TrackError> {
    let canonical_name_ref = canonical_name.as_str();
    let rows = sqlx::query_as!(
        records::ProjectAliasRow,
        r#"
        SELECT alias AS "alias!"
        FROM project_aliases
        WHERE canonical_name = ?1
        ORDER BY alias ASC
        "#,
        canonical_name_ref,
    )
    .fetch_all(&mut *connection)
    .await
    .database_error_with(format!(
        "Could not load project aliases for {canonical_name}"
    ))?;

    Ok(rows
        .into_iter()
        .map(|row| ProjectId::from_db(row.alias))
        .collect())
}

async fn ensure_aliases_are_available(
    connection: &mut sqlx::SqliteConnection,
    canonical_name: &ProjectId,
    aliases: &[ProjectId],
) -> Result<(), TrackError> {
    for alias in aliases {
        let alias_name = alias.as_str();
        let row = sqlx::query_as!(
            records::AliasOwnerRow,
            r#"
            SELECT canonical_name AS "canonical_name!"
            FROM project_aliases
            WHERE alias = ?1
            "#,
            alias_name,
        )
        .fetch_optional(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not verify whether alias {alias} is available"
        ))?;

        if let Some(row) = row {
            let claimed_by = ProjectId::from_db(row.canonical_name);
            if claimed_by != *canonical_name {
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
    use track_types::git_remote::GitRemote;
    use track_types::ids::ProjectId;
    use track_types::urls::Url;

    use crate::database::DatabaseContext;
    use crate::test_support::project_metadata;
    use track_projects::project_metadata::ProjectUpsertInput;

    #[tokio::test]
    async fn upsert_project_preserves_existing_aliases_when_no_new_aliases_are_provided() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![ProjectId::new("legacy-a").unwrap()],
            )
            .await
            .expect("project should save");
        let project = repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                Vec::new(),
            )
            .await
            .expect("project should update");

        assert_eq!(project.aliases, vec![ProjectId::new("legacy-a").unwrap()]);
        assert_eq!(
            project.metadata.description.as_deref(),
            Some("Metadata for project-a"),
        );
    }

    #[tokio::test]
    async fn upsert_project_unions_new_aliases_with_existing_aliases() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![ProjectId::new("legacy-a").unwrap()],
            )
            .await
            .expect("project should save");
        let project = repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![
                    ProjectId::new("new-a").unwrap(),
                    ProjectId::new("legacy-a").unwrap(),
                ],
            )
            .await
            .expect("project should update");

        assert_eq!(
            project.aliases,
            vec![
                ProjectId::new("legacy-a").unwrap(),
                ProjectId::new("new-a").unwrap(),
            ]
        );
    }

    #[tokio::test]
    async fn upsert_project_rejects_conflicting_alias_without_partial_writes() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![ProjectId::new("shared").unwrap()],
            )
            .await
            .expect("project a should save");
        repository
            .upsert_project_by_name(
                &ProjectId::new("project-b").unwrap(),
                project_metadata("project-b"),
                Vec::new(),
            )
            .await
            .expect("project b should save");

        let error = repository
            .upsert_project_by_name(
                &ProjectId::new("project-b").unwrap(),
                project_metadata("project-b"),
                vec![ProjectId::new("shared").unwrap()],
            )
            .await
            .expect_err("conflicting alias should fail");
        assert_eq!(error.code, ErrorCode::InvalidProjectMetadata);

        let project_b = repository
            .get_project_by_name(&ProjectId::new("project-b").unwrap())
            .await
            .expect("project b should still load");
        assert!(project_b.aliases.is_empty());
        assert_eq!(
            project_b.metadata.description.as_deref(),
            Some("Metadata for project-b"),
        );
    }

    #[tokio::test]
    async fn list_projects_returns_canonical_order_with_aliases() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        repository
            .upsert_project_by_name(
                &ProjectId::new("project-b").unwrap(),
                project_metadata("project-b"),
                vec![ProjectId::new("beta").unwrap()],
            )
            .await
            .expect("project b should save");
        repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![
                    ProjectId::new("alpha-2").unwrap(),
                    ProjectId::new("alpha-1").unwrap(),
                ],
            )
            .await
            .expect("project a should save");

        let projects = repository
            .list_projects()
            .await
            .expect("project list should load");

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].canonical_name, "project-a");
        assert_eq!(
            projects[0].aliases,
            vec![
                ProjectId::new("alpha-1").unwrap(),
                ProjectId::new("alpha-2").unwrap(),
            ],
        );
        assert_eq!(projects[1].canonical_name, "project-b");
        assert_eq!(projects[1].aliases, vec![ProjectId::new("beta").unwrap()]);
    }

    #[tokio::test]
    async fn update_project_by_name_keeps_aliases_while_replacing_metadata() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        repository
            .upsert_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                project_metadata("project-a"),
                vec![ProjectId::new("legacy-a").unwrap()],
            )
            .await
            .expect("project should save");

        let updated_metadata = track_projects::project_metadata::ProjectMetadata {
            repo_url: Url::parse("https://example.com/project-a").unwrap(),
            git_url: GitRemote::new("ssh://git@example.com/project-a.git").unwrap(),
            base_branch: "stable".to_owned(),
            description: Some("Updated metadata".to_owned()),
        };
        let project = repository
            .update_project_by_name(
                &ProjectId::new("project-a").unwrap(),
                updated_metadata.clone(),
            )
            .await
            .expect("project should update");

        assert_eq!(project.aliases, vec![ProjectId::new("legacy-a").unwrap()]);
        assert_eq!(project.metadata, updated_metadata);
    }

    #[tokio::test]
    async fn upsert_project_validates_and_persists_aliases() {
        let directory = TempDir::new().expect("tempdir should be created");
        let database = DatabaseContext::initialized(Some(directory.path().join("track.sqlite")))
            .await
            .expect("database should resolve");
        let repository = database.project_repository();

        let saved = repository
            .upsert_project(ProjectUpsertInput {
                canonical_name: ProjectId::new("project-a").unwrap(),
                aliases: vec![
                    ProjectId::new("alias-b").unwrap(),
                    ProjectId::new("alias-a").unwrap(),
                    ProjectId::new("alias-a").unwrap(),
                ],
                metadata: track_projects::project_metadata::ProjectMetadataUpdateInput {
                    repo_url: " https://github.com/acme/project-a ".to_owned(),
                    git_url: " git@github.com:acme/project-a.git ".to_owned(),
                    base_branch: " main ".to_owned(),
                    description: Some(" Primary project ".to_owned()),
                },
            })
            .await
            .expect("project should save");

        assert_eq!(saved.canonical_name, "project-a");
        assert_eq!(
            saved.aliases,
            vec![
                ProjectId::new("alias-a").unwrap(),
                ProjectId::new("alias-b").unwrap(),
            ],
        );
        assert_eq!(
            saved.metadata.repo_url.as_str(),
            "https://github.com/acme/project-a"
        );
        assert_eq!(
            saved.metadata.git_url.into_remote_string(),
            "git@github.com:acme/project-a.git"
        );
        assert_eq!(saved.metadata.base_branch, "main");
        assert_eq!(
            saved.metadata.description.as_deref(),
            Some("Primary project")
        );
    }
}
