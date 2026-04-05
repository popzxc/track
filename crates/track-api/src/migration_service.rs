// TODO: Kill it with fire

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use time::OffsetDateTime;
use track_config::config::{ConfigService, RemoteAgentConfigFile, TrackConfigFile};
use track_config::paths::{
    collapse_home_path, get_backend_managed_remote_agent_key_path,
    get_backend_managed_remote_agent_known_hosts_path, get_legacy_config_path, get_legacy_root_dir,
    path_to_string,
};
use track_dal::database::{DatabaseContext, DatabaseResultExt};
use track_projects::project_discovery::discover_projects_from_roots;
use track_projects::project_metadata::{infer_project_metadata, ProjectMetadata};
use track_types::errors::{ErrorCode, TrackError};
use track_types::migration::{
    CleanupCandidate, LegacyScanSummary, MigrationImportSummary, MigrationState, MigrationStatus,
    SkippedLegacyRecord, MIGRATION_STATUS_SETTING_KEY,
};
use track_types::path_component::validate_single_normal_path_component;
use track_types::time_utils::{format_iso_8601_millis, parse_iso_8601_millis};
use track_types::types::{
    Priority, ReviewRecord, ReviewRunRecord, Status, Task, TaskDispatchRecord, TaskSource,
};

use crate::backend_config::{RemoteAgentConfigService, REMOTE_AGENT_SETTING_KEY};

const LEGACY_ISSUES_DIR_NAME: &str = "issues";
const LEGACY_REVIEWS_DIR_NAME: &str = "reviews";
const LEGACY_REMOTE_AGENT_DIR_NAME: &str = "remote-agent";
const LEGACY_PROJECT_METADATA_FILE_NAME: &str = "PROJECT.md";

pub struct MigrationService {
    database: DatabaseContext,
    remote_agent_config_service: RemoteAgentConfigService,
    legacy_config_service: ConfigService,
    legacy_root: PathBuf,
}

impl MigrationService {
    pub fn new(
        remote_agent_config_service: RemoteAgentConfigService,
        database: DatabaseContext,
    ) -> Result<Self, TrackError> {
        Ok(Self {
            database,
            remote_agent_config_service,
            legacy_config_service: ConfigService::new(Some(get_legacy_config_path()?))?,
            legacy_root: get_legacy_root_dir()?,
        })
    }

    pub async fn status(&self) -> Result<MigrationStatus, TrackError> {
        let saved = self
            .remote_agent_config_service
            .load_migration_status()
            .await?;
        // Only a completed import is durable state now. Older saved `skipped`
        // values are treated like `ready` so legacy data still requires import.
        if saved.state == MigrationState::Imported {
            return Ok(saved);
        }

        if !self.database_is_empty().await? {
            return Ok(MigrationStatus::ready());
        }

        let snapshot = self.scan_legacy()?;
        if !snapshot.legacy_detected {
            return Ok(MigrationStatus::ready());
        }

        Ok(MigrationStatus {
            state: MigrationState::ImportRequired,
            requires_migration: true,
            can_import: true,
            legacy_detected: true,
            summary: snapshot.summary,
            skipped_records: snapshot.skipped_records,
            cleanup_candidates: snapshot.cleanup_candidates,
        })
    }

    pub async fn import_legacy(&self) -> Result<MigrationImportSummary, TrackError> {
        if !self.database_is_empty().await? {
            return Err(TrackError::new(
                ErrorCode::MigrationFailed,
                "The backend already contains data, so legacy import is only allowed into an empty SQLite database.",
            ));
        }

        let snapshot = self.scan_legacy()?;
        if !snapshot.legacy_detected {
            return Err(TrackError::new(
                ErrorCode::MigrationFailed,
                "No legacy track data was found to import.",
            ));
        }

        let imported_projects = snapshot.projects.clone();
        let imported_aliases = snapshot.aliases_by_project.clone();
        let imported_tasks = snapshot.tasks.clone();
        let imported_reviews = snapshot.reviews.clone();
        let imported_task_dispatches = snapshot.task_dispatches.clone();
        let imported_review_runs = snapshot.review_runs.clone();
        let imported_remote_agent_config = snapshot.remote_agent_config.clone();
        let skipped_records = snapshot.skipped_records.clone();
        let cleanup_candidates = snapshot.cleanup_candidates.clone();
        let summary = snapshot.summary.clone();
        let legacy_root = self.legacy_root.clone();

        let mut transaction = self.database.begin().await?;
        let mut copied_secret_files = Vec::new();
        let import_result = async {
            for project in &imported_projects {
                let aliases = imported_aliases
                    .get(&project.canonical_name)
                    .cloned()
                    .unwrap_or_default();
                import_project(&mut *transaction, project, aliases).await?;
            }

            for task in &imported_tasks {
                import_task(&mut *transaction, task).await?;
            }

            for review in &imported_reviews {
                import_review(&mut *transaction, review).await?;
            }

            for dispatch in &imported_task_dispatches {
                import_task_dispatch(&mut *transaction, dispatch).await?;
            }

            for review_run in &imported_review_runs {
                import_review_run(&mut *transaction, review_run).await?;
            }

            if let Some(remote_agent) = imported_remote_agent_config.as_ref() {
                save_backend_setting_json(
                    &mut *transaction,
                    REMOTE_AGENT_SETTING_KEY,
                    remote_agent,
                )
                .await?;
            }

            copied_secret_files = copy_remote_agent_secret_files(&legacy_root)?;

            let imported_summary = MigrationImportSummary {
                imported_projects: imported_projects.len(),
                imported_aliases: imported_aliases.values().map(Vec::len).sum(),
                imported_tasks: imported_tasks.len(),
                imported_task_dispatches: imported_task_dispatches.len(),
                imported_reviews: imported_reviews.len(),
                imported_review_runs: imported_review_runs.len(),
                remote_agent_config_imported: imported_remote_agent_config.is_some(),
                copied_secret_files: copied_secret_files
                    .iter()
                    .map(|path| path_to_string(path))
                    .collect(),
                skipped_records: skipped_records.clone(),
                cleanup_candidates: cleanup_candidates.clone(),
            };

            save_backend_setting_json(
                &mut *transaction,
                MIGRATION_STATUS_SETTING_KEY,
                &MigrationStatus {
                    state: MigrationState::Imported,
                    requires_migration: false,
                    can_import: false,
                    legacy_detected: true,
                    summary,
                    skipped_records: imported_summary.skipped_records.clone(),
                    cleanup_candidates: imported_summary.cleanup_candidates.clone(),
                },
            )
            .await?;

            Ok(imported_summary)
        }
        .await;

        match import_result {
            Ok(summary) => {
                transaction
                    .commit()
                    .await
                    .database_error_with("Could not commit the legacy import transaction")?;
                Ok(summary)
            }
            Err(error) => {
                cleanup_copied_secret_files(&copied_secret_files);
                Err(error)
            }
        }
    }

    async fn database_is_empty(&self) -> Result<bool, TrackError> {
        Ok(self
            .database
            .project_repository()
            .list_projects()
            .await?
            .is_empty()
            && self
                .database
                .task_repository()
                .list_tasks(true, None)
                .await?
                .is_empty()
            && self
                .database
                .review_repository()
                .list_reviews()
                .await?
                .is_empty()
            && self
                .database
                .dispatch_repository()
                .list_dispatches(Some(1))
                .await?
                .is_empty()
            && self
                .database
                .review_dispatch_repository()
                .list_dispatches(Some(1))
                .await?
                .is_empty()
            && self
                .remote_agent_config_service
                .load_remote_agent_config()
                .await?
                .is_none())
    }

    fn scan_legacy(&self) -> Result<LegacyImportSnapshot, TrackError> {
        let issues_dir = self.legacy_root.join(LEGACY_ISSUES_DIR_NAME);
        let reviews_dir = self.legacy_root.join(LEGACY_REVIEWS_DIR_NAME);
        let task_dispatches_dir = issues_dir.join(".dispatches");
        let review_dispatches_dir = reviews_dir.join(".dispatches");
        let legacy_config = load_legacy_config(&self.legacy_config_service);
        let mut snapshot = LegacyImportSnapshot {
            legacy_detected: issues_dir.exists()
                || reviews_dir.exists()
                || self.legacy_root.join(LEGACY_REMOTE_AGENT_DIR_NAME).exists()
                || legacy_config.is_some(),
            aliases_by_project: BTreeMap::new(),
            projects: Vec::new(),
            tasks: Vec::new(),
            task_dispatches: Vec::new(),
            reviews: Vec::new(),
            review_runs: Vec::new(),
            remote_agent_config: legacy_config
                .as_ref()
                .and_then(|config| config.remote_agent.clone()),
            skipped_records: Vec::new(),
            cleanup_candidates: build_cleanup_candidates(
                &self.legacy_root,
                self.legacy_config_service.resolved_path(),
            ),
            summary: LegacyScanSummary::default(),
        };

        if issues_dir.is_dir() {
            for entry in fs::read_dir(&issues_dir).map_err(|error| {
                TrackError::new(
                    ErrorCode::MigrationFailed,
                    format!(
                        "Could not read the legacy issues directory at {}: {error}",
                        path_to_string(&issues_dir)
                    ),
                )
            })? {
                let entry = entry.map_err(|error| {
                    TrackError::new(
                        ErrorCode::MigrationFailed,
                        format!(
                            "Could not read a legacy project entry under {}: {error}",
                            path_to_string(&issues_dir)
                        ),
                    )
                })?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(project_name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if project_name.starts_with('.') {
                    continue;
                }

                let canonical_name = match validate_single_normal_path_component(
                    project_name,
                    "Legacy project name",
                    ErrorCode::InvalidPathComponent,
                ) {
                    Ok(project_name) => project_name,
                    Err(error) => {
                        snapshot.skipped_records.push(SkippedLegacyRecord {
                            kind: "project".to_owned(),
                            path: path_to_string(&path),
                            error: error.to_string(),
                        });
                        continue;
                    }
                };

                let metadata = read_legacy_project_metadata(&path).unwrap_or_else(|error| {
                    snapshot.skipped_records.push(SkippedLegacyRecord {
                        kind: "project_metadata".to_owned(),
                        path: path_to_string(&path.join(LEGACY_PROJECT_METADATA_FILE_NAME)),
                        error: error.to_string(),
                    });
                    blank_project_metadata()
                });

                snapshot.projects.push(LegacyProjectImport {
                    canonical_name: canonical_name.clone(),
                    metadata,
                });

                for status in [Status::Open, Status::Closed] {
                    let status_dir = path.join(status.as_str());
                    if !status_dir.is_dir() {
                        continue;
                    }
                    for task_entry in fs::read_dir(&status_dir).map_err(|error| {
                        TrackError::new(
                            ErrorCode::MigrationFailed,
                            format!(
                                "Could not read the legacy task directory at {}: {error}",
                                path_to_string(&status_dir)
                            ),
                        )
                    })? {
                        let task_entry = task_entry.map_err(|error| {
                            TrackError::new(
                                ErrorCode::MigrationFailed,
                                format!(
                                    "Could not read a legacy task entry under {}: {error}",
                                    path_to_string(&status_dir)
                                ),
                            )
                        })?;
                        let task_path = task_entry.path();
                        if !task_path.is_file() {
                            continue;
                        }
                        match read_legacy_task_file(&issues_dir, &task_path) {
                            Ok(task) => snapshot.tasks.push(task),
                            Err(error) => snapshot.skipped_records.push(SkippedLegacyRecord {
                                kind: "task".to_owned(),
                                path: path_to_string(&task_path),
                                error: error.to_string(),
                            }),
                        }
                    }
                }
            }
        }

        if task_dispatches_dir.is_dir() {
            snapshot.task_dispatches = read_json_directory_tree::<TaskDispatchRecord>(
                &task_dispatches_dir,
                "task_dispatch",
                &mut snapshot.skipped_records,
            )?;
        }

        if reviews_dir.is_dir() {
            snapshot.reviews = read_json_directory_flat::<ReviewRecord>(
                &reviews_dir,
                "review",
                &mut snapshot.skipped_records,
            )?;
        }

        if review_dispatches_dir.is_dir() {
            snapshot.review_runs = read_json_directory_tree::<ReviewRunRecord>(
                &review_dispatches_dir,
                "review_run",
                &mut snapshot.skipped_records,
            )?;
        }

        if let Some(config) = legacy_config.as_ref() {
            merge_discovered_legacy_projects(
                &mut snapshot,
                config,
                self.legacy_config_service.resolved_path(),
                legacy_home_dir(&self.legacy_root),
            )?;
            attach_legacy_project_aliases(&mut snapshot, &config.project_aliases);
        }

        // Alias lists are persisted per project record, so we normalize them
        // after all project sources have been merged into one import snapshot.
        for aliases in snapshot.aliases_by_project.values_mut() {
            aliases.sort();
            aliases.dedup();
        }

        filter_orphaned_history(&mut snapshot);
        snapshot
            .projects
            .sort_by(|left, right| left.canonical_name.cmp(&right.canonical_name));
        snapshot.summary.projects_found = snapshot.projects.len();
        snapshot.summary.aliases_found = snapshot.aliases_by_project.values().map(Vec::len).sum();
        snapshot.summary.tasks_found = snapshot.tasks.len();
        snapshot.summary.task_dispatches_found = snapshot.task_dispatches.len();
        snapshot.summary.reviews_found = snapshot.reviews.len();
        snapshot.summary.review_runs_found = snapshot.review_runs.len();
        snapshot.summary.remote_agent_configured = snapshot.remote_agent_config.is_some();

        Ok(snapshot)
    }
}

fn filter_orphaned_history(snapshot: &mut LegacyImportSnapshot) {
    let imported_task_ids = snapshot
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    snapshot.task_dispatches.retain(|dispatch| {
        if imported_task_ids.contains(&dispatch.task_id) {
            return true;
        }

        snapshot.skipped_records.push(SkippedLegacyRecord {
            kind: "task_dispatch".to_owned(),
            path: dispatch.dispatch_id.clone(),
            error: format!(
                "Task dispatch references missing task {} and cannot be imported.",
                dispatch.task_id
            ),
        });
        false
    });

    let imported_review_ids = snapshot
        .reviews
        .iter()
        .map(|review| review.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    snapshot.review_runs.retain(|review_run| {
        if imported_review_ids.contains(&review_run.review_id) {
            return true;
        }

        snapshot.skipped_records.push(SkippedLegacyRecord {
            kind: "review_run".to_owned(),
            path: review_run.dispatch_id.clone(),
            error: format!(
                "Review run references missing review {} and cannot be imported.",
                review_run.review_id
            ),
        });
        false
    });
}

fn merge_discovered_legacy_projects(
    snapshot: &mut LegacyImportSnapshot,
    config: &TrackConfigFile,
    legacy_config_path: &Path,
    legacy_home_dir: &Path,
) -> Result<(), TrackError> {
    // Legacy CLI capture could target any discovered repository under the
    // configured project roots, even before that project had task files on
    // disk. The SQLite backend now owns the project registry, so we must
    // carry that discovered set forward or those repositories disappear until
    // the user re-registers them manually.
    let project_roots = config
        .project_roots
        .iter()
        .map(|value| {
            resolve_legacy_path_from_config_file(value, legacy_config_path, legacy_home_dir)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let discovered_projects =
        discover_projects_from_roots(&project_roots, &config.project_aliases)?;

    let mut imported_project_names = snapshot
        .projects
        .iter()
        .map(|project| project.canonical_name.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();

    for project in discovered_projects.into_projects() {
        let project_key = project.canonical_name.to_lowercase();
        if !imported_project_names.insert(project_key) {
            continue;
        }

        snapshot.projects.push(LegacyProjectImport {
            canonical_name: project.canonical_name.clone(),
            metadata: infer_project_metadata(&project),
        });
    }

    Ok(())
}

fn attach_legacy_project_aliases(
    snapshot: &mut LegacyImportSnapshot,
    configured_aliases: &BTreeMap<String, String>,
) {
    // Legacy discovery matched alias targets case-insensitively against the
    // discovered canonical project set. We keep that behavior here so config
    // values continue to mean the same thing during migration.
    let imported_project_names = snapshot
        .projects
        .iter()
        .map(|project| {
            (
                project.canonical_name.to_lowercase(),
                project.canonical_name.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for (alias, configured_canonical_name) in configured_aliases {
        if let Some(imported_canonical_name) =
            imported_project_names.get(&configured_canonical_name.to_lowercase())
        {
            snapshot
                .aliases_by_project
                .entry(imported_canonical_name.clone())
                .or_default()
                .push(alias.clone());
            continue;
        }

        snapshot.skipped_records.push(SkippedLegacyRecord {
            kind: "project_alias".to_owned(),
            path: format!("{alias} -> {configured_canonical_name}"),
            error: format!(
                "Legacy alias {alias} points to {configured_canonical_name}, but that project was not present in legacy issues or discovered from configured project roots and will not be imported."
            ),
        });
    }
}

fn legacy_home_dir(legacy_root: &Path) -> &Path {
    legacy_root.parent().unwrap_or(legacy_root)
}

fn resolve_legacy_path_from_config_file(
    path_value: &str,
    file_path: &Path,
    legacy_home_dir: &Path,
) -> Result<PathBuf, TrackError> {
    let base_dir = file_path.parent().ok_or_else(|| {
        TrackError::new(
            ErrorCode::InvalidConfig,
            format!(
                "Could not resolve a configured path relative to legacy config file {}.",
                collapse_home_path(file_path)
            ),
        )
    })?;

    let expanded = match path_value {
        "~" => legacy_home_dir.to_path_buf(),
        value if value.starts_with("~/") => legacy_home_dir.join(&value[2..]),
        value => PathBuf::from(value),
    };

    if expanded.is_absolute() {
        return Ok(expanded);
    }

    Ok(base_dir.join(expanded))
}

#[derive(Debug, Clone)]
struct LegacyImportSnapshot {
    legacy_detected: bool,
    aliases_by_project: BTreeMap<String, Vec<String>>,
    projects: Vec<LegacyProjectImport>,
    tasks: Vec<Task>,
    task_dispatches: Vec<TaskDispatchRecord>,
    reviews: Vec<ReviewRecord>,
    review_runs: Vec<ReviewRunRecord>,
    remote_agent_config: Option<RemoteAgentConfigFile>,
    skipped_records: Vec<SkippedLegacyRecord>,
    cleanup_candidates: Vec<CleanupCandidate>,
    summary: LegacyScanSummary,
}

#[derive(Debug, Clone)]
struct LegacyProjectImport {
    canonical_name: String,
    metadata: ProjectMetadata,
}

fn load_legacy_config(config_service: &ConfigService) -> Option<TrackConfigFile> {
    config_service.load_config_file().ok()
}

fn build_cleanup_candidates(
    legacy_root: &Path,
    legacy_config_path: &Path,
) -> Vec<CleanupCandidate> {
    let cleanup_targets = [
        (
            legacy_config_path.to_path_buf(),
            "Legacy shared config replaced by the CLI-only config file.",
        ),
        (
            legacy_root.join(LEGACY_ISSUES_DIR_NAME),
            "Legacy Markdown tasks were imported into the SQLite backend.",
        ),
        (
            legacy_root.join(LEGACY_REVIEWS_DIR_NAME),
            "Legacy review records were imported into the SQLite backend.",
        ),
        (
            legacy_root.join(LEGACY_REMOTE_AGENT_DIR_NAME),
            "Legacy managed remote-agent secrets were copied into backend state.",
        ),
    ];

    let mut candidates = Vec::new();
    for (path, reason) in cleanup_targets {
        if !path.exists() {
            continue;
        }

        candidates.push(CleanupCandidate {
            path: display_cleanup_candidate_path(&path),
            reason: reason.to_owned(),
        });
    }

    candidates
}

fn display_cleanup_candidate_path(path: &Path) -> String {
    let legacy_home_mount = Path::new("/home/track/legacy-home");
    if let Ok(relative) = path.strip_prefix(legacy_home_mount) {
        if relative.as_os_str().is_empty() {
            return "~".to_owned();
        }

        return format!("~/{}", path_to_string(relative).trim_start_matches('/'));
    }

    collapse_home_path(path)
}

fn copy_remote_agent_secret_files(legacy_root: &Path) -> Result<Vec<PathBuf>, TrackError> {
    let legacy_remote_agent_dir = legacy_root.join(LEGACY_REMOTE_AGENT_DIR_NAME);
    if !legacy_remote_agent_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut copied = Vec::new();
    let targets = [
        (
            legacy_remote_agent_dir.join("id_ed25519"),
            get_backend_managed_remote_agent_key_path()?,
        ),
        (
            legacy_remote_agent_dir.join("known_hosts"),
            get_backend_managed_remote_agent_known_hosts_path()?,
        ),
    ];

    for (source, destination) in targets {
        if !source.exists() {
            continue;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                cleanup_copied_secret_files(&copied);
                TrackError::new(
                    ErrorCode::MigrationFailed,
                    format!(
                        "Could not create the backend secrets directory at {}: {error}",
                        path_to_string(parent)
                    ),
                )
            })?;
        }

        fs::copy(&source, &destination).map_err(|error| {
            cleanup_copied_secret_files(&copied);
            TrackError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "Could not copy the legacy secret file from {} to {}: {error}",
                    path_to_string(&source),
                    path_to_string(&destination)
                ),
            )
        })?;
        copied.push(destination);
    }

    Ok(copied)
}

fn cleanup_copied_secret_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

async fn import_project(
    connection: &mut SqliteConnection,
    project: &LegacyProjectImport,
    aliases: Vec<String>,
) -> Result<(), TrackError> {
    let canonical_name = validate_single_normal_path_component(
        &project.canonical_name,
        "Project canonical name",
        ErrorCode::InvalidPathComponent,
    )?;

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
    .bind(&project.metadata.repo_url)
    .bind(&project.metadata.git_url)
    .bind(&project.metadata.base_branch)
    .bind(project.metadata.description.as_deref())
    .execute(&mut *connection)
    .await
    .database_error_with(format!("Could not import project {canonical_name}"))?;

    sqlx::query("DELETE FROM project_aliases WHERE canonical_name = ?1")
        .bind(&canonical_name)
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not replace project aliases for {canonical_name}"
        ))?;

    for alias in aliases {
        sqlx::query(
            r#"
            INSERT INTO project_aliases (canonical_name, alias)
            VALUES (?1, ?2)
            "#,
        )
        .bind(&canonical_name)
        .bind(&alias)
        .execute(&mut *connection)
        .await
        .database_error_with(format!(
            "Could not save the alias {alias} for project {canonical_name}"
        ))?;
    }

    Ok(())
}

async fn import_task(connection: &mut SqliteConnection, task: &Task) -> Result<(), TrackError> {
    sqlx::query(
        r#"
        INSERT INTO tasks (id, project, priority, status, description, created_at, updated_at, source)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            project = excluded.project,
            priority = excluded.priority,
            status = excluded.status,
            description = excluded.description,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            source = excluded.source
        "#,
    )
    .bind(&task.id)
    .bind(&task.project)
    .bind(task.priority.as_str())
    .bind(task.status.as_str())
    .bind(&task.description)
    .bind(format_iso_8601_millis(task.created_at))
    .bind(format_iso_8601_millis(task.updated_at))
    .bind(task.source.map(task_source_as_str))
    .execute(&mut *connection)
    .await
    .database_error_with(format!("Could not import task {}", task.id))?;

    Ok(())
}

async fn import_review(
    connection: &mut SqliteConnection,
    review: &ReviewRecord,
) -> Result<(), TrackError> {
    sqlx::query(
        r#"
        INSERT INTO reviews (
            id, pull_request_url, pull_request_number, pull_request_title,
            repository_full_name, repo_url, git_url, base_branch, workspace_key,
            project, main_user, default_review_prompt, extra_instructions,
            created_at, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT(id) DO UPDATE SET
            pull_request_url = excluded.pull_request_url,
            pull_request_number = excluded.pull_request_number,
            pull_request_title = excluded.pull_request_title,
            repository_full_name = excluded.repository_full_name,
            repo_url = excluded.repo_url,
            git_url = excluded.git_url,
            base_branch = excluded.base_branch,
            workspace_key = excluded.workspace_key,
            project = excluded.project,
            main_user = excluded.main_user,
            default_review_prompt = excluded.default_review_prompt,
            extra_instructions = excluded.extra_instructions,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&review.id)
    .bind(&review.pull_request_url)
    .bind(review.pull_request_number as i64)
    .bind(&review.pull_request_title)
    .bind(&review.repository_full_name)
    .bind(&review.repo_url)
    .bind(&review.git_url)
    .bind(&review.base_branch)
    .bind(&review.workspace_key)
    .bind(review.project.as_deref())
    .bind(&review.main_user)
    .bind(review.default_review_prompt.as_deref())
    .bind(review.extra_instructions.as_deref())
    .bind(format_iso_8601_millis(review.created_at))
    .bind(format_iso_8601_millis(review.updated_at))
    .execute(&mut *connection)
    .await
    .database_error_with(format!("Could not import review {}", review.id))?;

    Ok(())
}

async fn import_task_dispatch(
    connection: &mut SqliteConnection,
    dispatch: &TaskDispatchRecord,
) -> Result<(), TrackError> {
    validate_single_normal_path_component(
        &dispatch.dispatch_id,
        "Dispatch id",
        ErrorCode::InvalidPathComponent,
    )?;
    validate_single_normal_path_component(
        &dispatch.task_id,
        "Task id",
        ErrorCode::InvalidPathComponent,
    )?;

    sqlx::query(
        r#"
        INSERT INTO task_dispatches (
            dispatch_id, task_id, project, status, created_at, updated_at, finished_at,
            remote_host, branch_name, worktree_path, pull_request_url, follow_up_request,
            summary, notes, error_message, review_request_head_oid, review_request_user
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        ON CONFLICT(dispatch_id) DO UPDATE SET
            task_id = excluded.task_id,
            project = excluded.project,
            status = excluded.status,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            finished_at = excluded.finished_at,
            remote_host = excluded.remote_host,
            branch_name = excluded.branch_name,
            worktree_path = excluded.worktree_path,
            pull_request_url = excluded.pull_request_url,
            follow_up_request = excluded.follow_up_request,
            summary = excluded.summary,
            notes = excluded.notes,
            error_message = excluded.error_message,
            review_request_head_oid = excluded.review_request_head_oid,
            review_request_user = excluded.review_request_user
        "#,
    )
    .bind(&dispatch.dispatch_id)
    .bind(&dispatch.task_id)
    .bind(&dispatch.project)
    .bind(dispatch.status.as_str())
    .bind(format_iso_8601_millis(dispatch.created_at))
    .bind(format_iso_8601_millis(dispatch.updated_at))
    .bind(dispatch.finished_at.map(format_iso_8601_millis))
    .bind(&dispatch.remote_host)
    .bind(dispatch.branch_name.as_deref())
    .bind(dispatch.worktree_path.as_deref())
    .bind(dispatch.pull_request_url.as_deref())
    .bind(dispatch.follow_up_request.as_deref())
    .bind(dispatch.summary.as_deref())
    .bind(dispatch.notes.as_deref())
    .bind(dispatch.error_message.as_deref())
    .bind(dispatch.review_request_head_oid.as_deref())
    .bind(dispatch.review_request_user.as_deref())
    .execute(&mut *connection)
    .await
    .database_error_with(format!(
        "Could not import the dispatch record for task {}",
        dispatch.task_id
    ))?;

    Ok(())
}

async fn import_review_run(
    connection: &mut SqliteConnection,
    review_run: &ReviewRunRecord,
) -> Result<(), TrackError> {
    validate_single_normal_path_component(
        &review_run.review_id,
        "Review id",
        ErrorCode::InvalidPathComponent,
    )?;
    validate_single_normal_path_component(
        &review_run.dispatch_id,
        "Dispatch id",
        ErrorCode::InvalidPathComponent,
    )?;

    sqlx::query(
        r#"
        INSERT INTO review_runs (
            dispatch_id, review_id, pull_request_url, repository_full_name,
            workspace_key, status, created_at, updated_at, finished_at,
            remote_host, branch_name, worktree_path, follow_up_request,
            target_head_oid, summary, review_submitted, github_review_id,
            github_review_url, notes, error_message
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        ON CONFLICT(dispatch_id) DO UPDATE SET
            review_id = excluded.review_id,
            pull_request_url = excluded.pull_request_url,
            repository_full_name = excluded.repository_full_name,
            workspace_key = excluded.workspace_key,
            status = excluded.status,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            finished_at = excluded.finished_at,
            remote_host = excluded.remote_host,
            branch_name = excluded.branch_name,
            worktree_path = excluded.worktree_path,
            follow_up_request = excluded.follow_up_request,
            target_head_oid = excluded.target_head_oid,
            summary = excluded.summary,
            review_submitted = excluded.review_submitted,
            github_review_id = excluded.github_review_id,
            github_review_url = excluded.github_review_url,
            notes = excluded.notes,
            error_message = excluded.error_message
        "#,
    )
    .bind(&review_run.dispatch_id)
    .bind(&review_run.review_id)
    .bind(&review_run.pull_request_url)
    .bind(&review_run.repository_full_name)
    .bind(&review_run.workspace_key)
    .bind(review_run.status.as_str())
    .bind(format_iso_8601_millis(review_run.created_at))
    .bind(format_iso_8601_millis(review_run.updated_at))
    .bind(review_run.finished_at.map(format_iso_8601_millis))
    .bind(&review_run.remote_host)
    .bind(review_run.branch_name.as_deref())
    .bind(review_run.worktree_path.as_deref())
    .bind(review_run.follow_up_request.as_deref())
    .bind(review_run.target_head_oid.as_deref())
    .bind(review_run.summary.as_deref())
    .bind(review_run.review_submitted as i64)
    .bind(review_run.github_review_id.as_deref())
    .bind(review_run.github_review_url.as_deref())
    .bind(review_run.notes.as_deref())
    .bind(review_run.error_message.as_deref())
    .execute(&mut *connection)
    .await
    .database_error_with(format!(
        "Could not import the review run record for review {}",
        review_run.review_id
    ))?;

    Ok(())
}

async fn save_backend_setting_json<T>(
    connection: &mut SqliteConnection,
    key: &str,
    value: &T,
) -> Result<(), TrackError>
where
    T: Serialize,
{
    let serialized = serde_json::to_string(value).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfig,
            format!("Could not serialize backend setting `{key}`: {error}"),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO backend_settings (setting_key, setting_json)
        VALUES (?1, ?2)
        ON CONFLICT(setting_key) DO UPDATE SET setting_json = excluded.setting_json
        "#,
    )
    .bind(key)
    .bind(&serialized)
    .execute(&mut *connection)
    .await
    .database_error_with(format!("Could not save backend setting `{key}`"))?;

    Ok(())
}

fn task_source_as_str(source: TaskSource) -> &'static str {
    match source {
        TaskSource::Cli => "cli",
        TaskSource::Web => "web",
    }
}

fn read_json_directory_tree<T>(
    root: &Path,
    kind: &str,
    skipped_records: &mut Vec<SkippedLegacyRecord>,
) -> Result<Vec<T>, TrackError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut records = Vec::new();
    for parent in fs::read_dir(root).map_err(|error| {
        TrackError::new(
            ErrorCode::MigrationFailed,
            format!(
                "Could not read the legacy directory at {}: {error}",
                path_to_string(root)
            ),
        )
    })? {
        let parent = parent.map_err(|error| {
            TrackError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "Could not read a legacy entry under {}: {error}",
                    path_to_string(root)
                ),
            )
        })?;
        let parent_path = parent.path();
        if !parent_path.is_dir() {
            continue;
        }

        for child in fs::read_dir(&parent_path).map_err(|error| {
            TrackError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "Could not read the legacy directory at {}: {error}",
                    path_to_string(&parent_path)
                ),
            )
        })? {
            let child = child.map_err(|error| {
                TrackError::new(
                    ErrorCode::MigrationFailed,
                    format!(
                        "Could not read a legacy entry under {}: {error}",
                        path_to_string(&parent_path)
                    ),
                )
            })?;
            let child_path = child.path();
            if !child_path.is_file() {
                continue;
            }
            match fs::read_to_string(&child_path)
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::MigrationFailed,
                        format!(
                            "Could not read the legacy file at {}: {error}",
                            path_to_string(&child_path)
                        ),
                    )
                })
                .and_then(|raw| {
                    serde_json::from_str::<T>(&raw).map_err(|error| {
                        TrackError::new(
                            ErrorCode::MigrationFailed,
                            format!(
                                "Legacy JSON at {} is malformed: {error}",
                                path_to_string(&child_path)
                            ),
                        )
                    })
                }) {
                Ok(record) => records.push(record),
                Err(error) => skipped_records.push(SkippedLegacyRecord {
                    kind: kind.to_owned(),
                    path: path_to_string(&child_path),
                    error: error.to_string(),
                }),
            }
        }
    }

    Ok(records)
}

fn read_json_directory_flat<T>(
    root: &Path,
    kind: &str,
    skipped_records: &mut Vec<SkippedLegacyRecord>,
) -> Result<Vec<T>, TrackError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut records = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| {
        TrackError::new(
            ErrorCode::MigrationFailed,
            format!(
                "Could not read the legacy directory at {}: {error}",
                path_to_string(root)
            ),
        )
    })? {
        let entry = entry.map_err(|error| {
            TrackError::new(
                ErrorCode::MigrationFailed,
                format!(
                    "Could not read a legacy entry under {}: {error}",
                    path_to_string(root)
                ),
            )
        })?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path)
            .map_err(|error| {
                TrackError::new(
                    ErrorCode::MigrationFailed,
                    format!(
                        "Could not read the legacy file at {}: {error}",
                        path_to_string(&path)
                    ),
                )
            })
            .and_then(|raw| {
                serde_json::from_str::<T>(&raw).map_err(|error| {
                    TrackError::new(
                        ErrorCode::MigrationFailed,
                        format!(
                            "Legacy JSON at {} is malformed: {error}",
                            path_to_string(&path)
                        ),
                    )
                })
            }) {
            Ok(record) => records.push(record),
            Err(error) => skipped_records.push(SkippedLegacyRecord {
                kind: kind.to_owned(),
                path: path_to_string(&path),
                error: error.to_string(),
            }),
        }
    }

    Ok(records)
}

fn read_legacy_project_metadata(project_directory: &Path) -> Result<ProjectMetadata, TrackError> {
    let metadata_path = project_directory.join(LEGACY_PROJECT_METADATA_FILE_NAME);
    if !metadata_path.exists() {
        return Ok(blank_project_metadata());
    }

    let raw_file = fs::read_to_string(&metadata_path).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidProjectMetadata,
            format!(
                "Could not read the legacy project metadata file at {}: {error}",
                path_to_string(&metadata_path)
            ),
        )
    })?;
    let (frontmatter, body) = split_frontmatter_sections(&raw_file)?;
    let parsed =
        serde_yaml::from_str::<ParsedProjectMetadataFrontmatter>(frontmatter).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidProjectMetadata,
                format!(
                    "Project metadata at {} has invalid YAML frontmatter: {error}",
                    path_to_string(&metadata_path)
                ),
            )
        })?;

    Ok(ProjectMetadata {
        repo_url: required_string(parsed.repo_url, "repoUrl", &metadata_path)?,
        git_url: required_string(parsed.git_url, "gitUrl", &metadata_path)?,
        base_branch: required_string(parsed.base_branch, "baseBranch", &metadata_path)?,
        description: (!body.trim().is_empty()).then_some(body.trim().to_owned()),
    })
}

fn read_legacy_task_file(issues_dir: &Path, file_path: &Path) -> Result<Task, TrackError> {
    let raw_file = fs::read_to_string(file_path).map_err(|error| {
        TrackError::new(
            ErrorCode::TaskWriteFailed,
            format!(
                "Could not read task file at {}: {error}",
                path_to_string(file_path)
            ),
        )
    })?;
    let path_metadata = parse_legacy_task_path(issues_dir, file_path)?;
    let (frontmatter, body) = split_frontmatter_sections(&raw_file)?;
    let parsed = serde_yaml::from_str::<ParsedTaskFrontmatter>(frontmatter).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!("Could not parse task frontmatter: {error}"),
        )
    })?;

    let created_at = required_timestamp(parsed.created_at, "createdAt")?;
    let updated_at = required_timestamp(parsed.updated_at, "updatedAt")?;
    let description = body.trim().to_owned();
    if description.is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Task Markdown body is empty.",
        ));
    }

    Ok(Task {
        id: path_metadata.id,
        project: path_metadata.project,
        priority: parsed.priority.ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                "Task frontmatter is missing required field priority.",
            )
        })?,
        status: path_metadata.status,
        description,
        created_at,
        updated_at,
        source: parsed.source,
    })
}

fn parse_legacy_task_path(
    issues_dir: &Path,
    file_path: &Path,
) -> Result<LegacyTaskPathMetadata, TrackError> {
    let relative_path = file_path.strip_prefix(issues_dir).map_err(|_| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Task file path {} is outside the configured data directory.",
                path_to_string(file_path)
            ),
        )
    })?;
    let mut components = relative_path.components();
    let project = component_as_string(components.next(), "project", file_path)?;
    let status = parse_status_component(components.next(), file_path)?;
    let file_name = component_as_string(components.next(), "task filename", file_path)?;
    if components.next().is_some() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Task file path {} does not match the expected project/status/id.md layout.",
                path_to_string(file_path)
            ),
        ));
    }
    let id = file_name
        .strip_suffix(".md")
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} is missing the task identifier in its filename.",
                    path_to_string(file_path)
                ),
            )
        })?;

    Ok(LegacyTaskPathMetadata {
        id,
        project,
        status,
    })
}

fn blank_project_metadata() -> ProjectMetadata {
    ProjectMetadata {
        repo_url: String::new(),
        git_url: String::new(),
        base_branch: "main".to_owned(),
        description: None,
    }
}

#[derive(Debug, Deserialize)]
struct ParsedTaskFrontmatter {
    priority: Option<Priority>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    source: Option<TaskSource>,
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

#[derive(Debug)]
struct LegacyTaskPathMetadata {
    id: String,
    project: String,
    status: Status,
}

fn required_timestamp(
    value: Option<String>,
    field_name: &str,
) -> Result<OffsetDateTime, TrackError> {
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!("Task frontmatter is missing required field {field_name}."),
            )
        })?;

    parse_iso_8601_millis(&value).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!("Task frontmatter field {field_name} is not a valid timestamp: {error}"),
        )
    })
}

fn required_string(
    value: Option<String>,
    field_name: &str,
    file_path: &Path,
) -> Result<String, TrackError> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidProjectMetadata,
                format!(
                    "Project metadata at {} is missing required field {field_name}.",
                    path_to_string(file_path)
                ),
            )
        })
}

fn split_frontmatter_sections(raw_file: &str) -> Result<(&str, &str), TrackError> {
    let Some(after_start) = consume_frontmatter_delimiter(raw_file, 0) else {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Legacy Markdown file must start with YAML frontmatter.",
        ));
    };
    let Some(end_start) = find_frontmatter_end(raw_file, after_start) else {
        return Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Legacy Markdown file is missing the closing YAML frontmatter delimiter.",
        ));
    };
    let frontmatter = &raw_file[after_start..end_start];
    let body_start = consume_frontmatter_delimiter(raw_file, end_start).ok_or_else(|| {
        TrackError::new(
            ErrorCode::InvalidConfigInput,
            "Legacy Markdown file is missing the closing YAML frontmatter delimiter.",
        )
    })?;
    Ok((frontmatter, &raw_file[body_start..]))
}

fn consume_frontmatter_delimiter(raw_file: &str, offset: usize) -> Option<usize> {
    match raw_file.get(offset..)? {
        rest if rest.starts_with("---\r\n") => Some(offset + 5),
        rest if rest.starts_with("---\n") => Some(offset + 4),
        _ => None,
    }
}

fn find_frontmatter_end(raw_file: &str, start: usize) -> Option<usize> {
    let bytes = raw_file.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        if (index == 0 || bytes.get(index.wrapping_sub(1)) == Some(&b'\n'))
            && raw_file.get(index..)?.starts_with("---")
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn component_as_string(
    component: Option<Component<'_>>,
    label: &str,
    file_path: &Path,
) -> Result<String, TrackError> {
    component
        .and_then(|component| component.as_os_str().to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            TrackError::new(
                ErrorCode::InvalidConfigInput,
                format!(
                    "Task file path {} is missing the {label} component.",
                    path_to_string(file_path)
                ),
            )
        })
}

fn parse_status_component(
    component: Option<Component<'_>>,
    file_path: &Path,
) -> Result<Status, TrackError> {
    let raw_status = component_as_string(component, "status", file_path)?;
    match raw_status.as_str() {
        "open" => Ok(Status::Open),
        "closed" => Ok(Status::Closed),
        _ => Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Task file path {} uses unsupported status directory {}.",
                path_to_string(file_path),
                raw_status
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{display_cleanup_candidate_path, MigrationService};
    use crate::backend_config::RemoteAgentConfigService;
    use crate::test_support::{set_env_var, track_data_env_lock, EnvVarGuard};
    use track_config::config::{ConfigService, TrackConfigFile};
    use track_config::paths::get_backend_database_path;
    use track_dal::database::DatabaseContext;
    use track_types::migration::MigrationState;

    struct TestEnvironment {
        _env_lock: std::sync::MutexGuard<'static, ()>,
        _track_data_dir_guard: EnvVarGuard,
        _track_state_dir_guard: EnvVarGuard,
        _track_legacy_root_guard: EnvVarGuard,
        _track_legacy_config_guard: EnvVarGuard,
    }

    impl TestEnvironment {
        fn new(directory: &TempDir) -> Self {
            let env_lock = track_data_env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let backend_state_dir = directory.path().join("backend");
            let backend_data_dir = backend_state_dir.join("issues");
            let legacy_root = directory.path().join("legacy-root");
            let legacy_config_path = directory.path().join("legacy-config/config.json");

            Self {
                _env_lock: env_lock,
                _track_data_dir_guard: set_env_var("TRACK_DATA_DIR", &backend_data_dir),
                _track_state_dir_guard: set_env_var("TRACK_STATE_DIR", &backend_state_dir),
                _track_legacy_root_guard: set_env_var("TRACK_LEGACY_ROOT", &legacy_root),
                _track_legacy_config_guard: set_env_var(
                    "TRACK_LEGACY_CONFIG_PATH",
                    &legacy_config_path,
                ),
            }
        }
    }

    async fn migration_service() -> MigrationService {
        let database = DatabaseContext::initialized(None)
            .await
            .expect("database should resolve");
        MigrationService::new(
            RemoteAgentConfigService::new(None)
                .await
                .expect("remote-agent config service should resolve"),
            database,
        )
        .expect("migration service should resolve")
    }

    #[tokio::test]
    async fn import_skips_orphaned_history_instead_of_aborting() {
        let directory = TempDir::new().expect("tempdir should be created");
        let _environment = TestEnvironment::new(&directory);
        let legacy_root = directory.path().join("legacy-root");
        let project_dir = legacy_root.join("issues/project-a");
        let open_dir = project_dir.join("open");
        let task_dispatch_dir = legacy_root.join("issues/.dispatches/project-a");
        let review_dir = legacy_root.join("reviews");
        let review_run_dir = review_dir.join(".dispatches/project-a");
        std::fs::create_dir_all(&open_dir).expect("legacy task directory should exist");
        std::fs::create_dir_all(&task_dispatch_dir)
            .expect("legacy dispatch directory should exist");
        std::fs::create_dir_all(&review_run_dir).expect("legacy review run directory should exist");

        std::fs::write(
            open_dir.join("20260323-fix-queue-layout.md"),
            "---\npriority: high\ncreatedAt: 2026-03-23T12:00:00.000Z\nupdatedAt: 2026-03-23T12:00:00.000Z\nsource: cli\n---\nFix queue layout\n",
        )
        .expect("valid task should be written");
        std::fs::write(
            open_dir.join("20260323-bad-task.md"),
            "---\npriority: high\ncreatedAt: nope\n---\nBroken task\n",
        )
        .expect("invalid task should be written");

        std::fs::write(
            task_dispatch_dir.join("valid.json"),
            serde_json::json!({
                "dispatchId": "dispatch-valid",
                "taskId": "20260323-fix-queue-layout",
                "project": "project-a",
                "status": "succeeded",
                "createdAt": "2026-03-23T12:05:00.000Z",
                "updatedAt": "2026-03-23T12:06:00.000Z",
                "finishedAt": "2026-03-23T12:06:00.000Z",
                "remoteHost": "192.0.2.25",
                "branchName": "track/dispatch-valid",
                "worktreePath": "/tmp/worktree",
                "summary": "Completed."
            })
            .to_string(),
        )
        .expect("valid dispatch should be written");
        std::fs::write(
            task_dispatch_dir.join("orphan.json"),
            serde_json::json!({
                "dispatchId": "dispatch-orphan",
                "taskId": "20260323-bad-task",
                "project": "project-a",
                "status": "failed",
                "createdAt": "2026-03-23T12:07:00.000Z",
                "updatedAt": "2026-03-23T12:08:00.000Z",
                "finishedAt": "2026-03-23T12:08:00.000Z",
                "remoteHost": "192.0.2.25",
                "summary": "Failed."
            })
            .to_string(),
        )
        .expect("orphan dispatch should be written");

        std::fs::write(
            review_dir.join("review-1.json"),
            serde_json::json!({
                "id": "review-1",
                "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
                "pullRequestNumber": 42,
                "pullRequestTitle": "Fix queue layout",
                "repositoryFullName": "acme/project-a",
                "repoUrl": "https://github.com/acme/project-a",
                "gitUrl": "git@github.com:acme/project-a.git",
                "baseBranch": "main",
                "workspaceKey": "project-a",
                "project": "project-a",
                "mainUser": "octocat",
                "createdAt": "2026-03-26T12:00:00.000Z",
                "updatedAt": "2026-03-26T12:00:00.000Z"
            })
            .to_string(),
        )
        .expect("review should be written");
        std::fs::write(
            review_run_dir.join("valid.json"),
            serde_json::json!({
                "dispatchId": "review-run-valid",
                "reviewId": "review-1",
                "pullRequestUrl": "https://github.com/acme/project-a/pull/42",
                "repositoryFullName": "acme/project-a",
                "workspaceKey": "project-a",
                "status": "succeeded",
                "createdAt": "2026-03-26T12:05:00.000Z",
                "updatedAt": "2026-03-26T12:06:00.000Z",
                "finishedAt": "2026-03-26T12:06:00.000Z",
                "remoteHost": "192.0.2.25",
                "branchName": "track-review/review-run-valid",
                "worktreePath": "/tmp/review-worktree",
                "summary": "Submitted review.",
                "reviewSubmitted": true
            })
            .to_string(),
        )
        .expect("valid review run should be written");
        std::fs::write(
            review_run_dir.join("orphan.json"),
            serde_json::json!({
                "dispatchId": "review-run-orphan",
                "reviewId": "review-missing",
                "pullRequestUrl": "https://github.com/acme/project-a/pull/43",
                "repositoryFullName": "acme/project-a",
                "workspaceKey": "project-a",
                "status": "failed",
                "createdAt": "2026-03-26T12:07:00.000Z",
                "updatedAt": "2026-03-26T12:08:00.000Z",
                "finishedAt": "2026-03-26T12:08:00.000Z",
                "remoteHost": "192.0.2.25",
                "summary": "Failed review."
            })
            .to_string(),
        )
        .expect("orphan review run should be written");

        let service = migration_service().await;
        let status = service
            .status()
            .await
            .expect("migration status should load");
        assert!(status.requires_migration);
        assert_eq!(status.summary.tasks_found, 1);
        assert_eq!(status.summary.task_dispatches_found, 1);
        assert_eq!(status.summary.reviews_found, 1);
        assert_eq!(status.summary.review_runs_found, 1);
        assert!(status
            .skipped_records
            .iter()
            .any(|record| record.kind == "task_dispatch"
                && record.error.contains("missing task 20260323-bad-task")));
        assert!(status
            .skipped_records
            .iter()
            .any(|record| record.kind == "review_run"
                && record.error.contains("missing review review-missing")));

        let summary = service
            .import_legacy()
            .await
            .expect("legacy import should succeed");
        assert_eq!(summary.imported_tasks, 1);
        assert_eq!(summary.imported_task_dispatches, 1);
        assert_eq!(summary.imported_reviews, 1);
        assert_eq!(summary.imported_review_runs, 1);
        assert!(summary
            .cleanup_candidates
            .iter()
            .any(|candidate| candidate.path.ends_with("legacy-root/issues")));

        let database = DatabaseContext::initialized(Some(
            get_backend_database_path().expect("database path should resolve"),
        ))
        .await
        .expect("database should resolve");
        let dispatches = database
            .dispatch_repository()
            .list_dispatches(None)
            .await
            .expect("dispatches should list");
        assert_eq!(dispatches.len(), 1);

        let review_runs = database
            .review_dispatch_repository()
            .list_dispatches(None)
            .await
            .expect("review runs should list");
        assert_eq!(review_runs.len(), 1);

        let post_import_status = service
            .status()
            .await
            .expect("migration status should reload");
        assert_eq!(post_import_status.state, MigrationState::Imported);
    }

    #[tokio::test]
    async fn migrates_configured_projects_without_issues_and_skips_alias_only_targets() {
        let directory = TempDir::new().expect("tempdir should be created");
        let _environment = TestEnvironment::new(&directory);
        let legacy_root = directory.path().join("legacy-root");
        let legacy_config_path = directory.path().join("legacy-config/config.json");
        let configured_repo_git_dir = directory.path().join("workspace/project-b/.git");
        std::fs::create_dir_all(legacy_root.join("issues/project-a/open"))
            .expect("legacy project directory should exist");
        std::fs::create_dir_all(&configured_repo_git_dir)
            .expect("configured project directory should exist");

        ConfigService::new(Some(legacy_config_path))
            .expect("legacy config service should resolve")
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["~/workspace".to_owned()],
                project_aliases: BTreeMap::from([
                    ("proj-a".to_owned(), "project-a".to_owned()),
                    ("proj-b".to_owned(), "Project-B".to_owned()),
                    ("ghost".to_owned(), "project-missing".to_owned()),
                ]),
                ..TrackConfigFile::default()
            })
            .expect("legacy config should save");

        let service = migration_service().await;
        let status = service
            .status()
            .await
            .expect("migration status should load");
        assert!(status.requires_migration);
        assert_eq!(status.summary.projects_found, 2);
        assert_eq!(status.summary.aliases_found, 2);
        assert!(status
            .skipped_records
            .iter()
            .any(|record| record.kind == "project_alias"
                && record.path == "ghost -> project-missing"));

        let summary = service
            .import_legacy()
            .await
            .expect("legacy import should succeed");
        assert_eq!(summary.imported_projects, 2);
        assert_eq!(summary.imported_aliases, 2);
        assert!(summary
            .skipped_records
            .iter()
            .any(|record| record.kind == "project_alias"
                && record.path == "ghost -> project-missing"));

        let database = DatabaseContext::initialized(Some(
            get_backend_database_path().expect("database path should resolve"),
        ))
        .await
        .expect("database should resolve");
        let imported_projects = database
            .project_repository()
            .list_projects()
            .await
            .expect("projects should list");
        assert_eq!(imported_projects.len(), 2);
        assert_eq!(imported_projects[0].canonical_name, "project-a");
        assert_eq!(imported_projects[0].aliases, vec!["proj-a".to_owned()]);
        assert_eq!(imported_projects[1].canonical_name, "project-b");
        assert_eq!(imported_projects[1].aliases, vec!["proj-b".to_owned()]);
    }

    #[test]
    fn cleanup_candidates_render_compose_mount_paths_as_host_paths() {
        assert_eq!(
            display_cleanup_candidate_path(Path::new("/home/track/legacy-home/.track/issues")),
            "~/.track/issues"
        );
        assert_eq!(
            display_cleanup_candidate_path(Path::new(
                "/home/track/legacy-home/.config/track/config.json"
            )),
            "~/.config/track/config.json"
        );
    }
}
