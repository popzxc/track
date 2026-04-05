use axum::Json;
use serde::Serialize;

// =============================================================================
// Frontend Migration Compatibility
// =============================================================================
//
// The legacy import flow is removed from the Rust backend, but the current
// frontend still knows how to call `/api/migration/*`. Keep those endpoints as
// inert compatibility shims until the UI drops them too, then delete this
// module outright.

pub(crate) async fn migration_status() -> Json<MigrationStatusResponse> {
    Json(MigrationStatusResponse {
        migration: MigrationStatus::imported(),
    })
}

pub(crate) async fn import_legacy_data() -> Json<MigrationImportResponse> {
    Json(MigrationImportResponse {
        summary: MigrationImportSummary::default(),
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationStatusResponse {
    migration: MigrationStatus,
}

#[derive(Debug, Serialize)]
pub(crate) struct MigrationImportResponse {
    summary: MigrationImportSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum MigrationState {
    Imported,
}

#[derive(Debug, Serialize, Default)]
struct LegacyScanSummary {
    #[serde(rename = "projectsFound")]
    projects_found: usize,
    #[serde(rename = "aliasesFound")]
    aliases_found: usize,
    #[serde(rename = "tasksFound")]
    tasks_found: usize,
    #[serde(rename = "taskDispatchesFound")]
    task_dispatches_found: usize,
    #[serde(rename = "reviewsFound")]
    reviews_found: usize,
    #[serde(rename = "reviewRunsFound")]
    review_runs_found: usize,
    #[serde(rename = "remoteAgentConfigured")]
    remote_agent_configured: bool,
}

#[derive(Debug, Serialize)]
struct SkippedLegacyRecord {
    kind: String,
    path: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct CleanupCandidate {
    path: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct MigrationStatus {
    state: MigrationState,
    #[serde(rename = "requiresMigration")]
    requires_migration: bool,
    #[serde(rename = "canImport")]
    can_import: bool,
    #[serde(rename = "legacyDetected")]
    legacy_detected: bool,
    summary: LegacyScanSummary,
    #[serde(rename = "skippedRecords")]
    skipped_records: Vec<SkippedLegacyRecord>,
    #[serde(rename = "cleanupCandidates")]
    cleanup_candidates: Vec<CleanupCandidate>,
}

impl MigrationStatus {
    fn imported() -> Self {
        Self {
            state: MigrationState::Imported,
            requires_migration: false,
            can_import: false,
            legacy_detected: false,
            summary: LegacyScanSummary::default(),
            skipped_records: Vec::new(),
            cleanup_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Default)]
struct MigrationImportSummary {
    #[serde(rename = "importedProjects")]
    imported_projects: usize,
    #[serde(rename = "importedAliases")]
    imported_aliases: usize,
    #[serde(rename = "importedTasks")]
    imported_tasks: usize,
    #[serde(rename = "importedTaskDispatches")]
    imported_task_dispatches: usize,
    #[serde(rename = "importedReviews")]
    imported_reviews: usize,
    #[serde(rename = "importedReviewRuns")]
    imported_review_runs: usize,
    #[serde(rename = "remoteAgentConfigImported")]
    remote_agent_config_imported: bool,
    #[serde(rename = "copiedSecretFiles")]
    copied_secret_files: Vec<String>,
    #[serde(rename = "skippedRecords")]
    skipped_records: Vec<SkippedLegacyRecord>,
    #[serde(rename = "cleanupCandidates")]
    cleanup_candidates: Vec<CleanupCandidate>,
}
