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
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationStatusResponse {
    migration: MigrationStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationImportResponse {
    summary: MigrationImportSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum MigrationState {
    Imported,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct LegacyScanSummary {
    projects_found: usize,
    aliases_found: usize,
    tasks_found: usize,
    task_dispatches_found: usize,
    reviews_found: usize,
    review_runs_found: usize,
    remote_agent_configured: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedLegacyRecord {
    kind: String,
    path: String,
    error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanupCandidate {
    path: String,
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationStatus {
    state: MigrationState,
    requires_migration: bool,
    can_import: bool,
    legacy_detected: bool,
    summary: LegacyScanSummary,
    skipped_records: Vec<SkippedLegacyRecord>,
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
#[serde(rename_all = "camelCase")]
struct MigrationImportSummary {
    imported_projects: usize,
    imported_aliases: usize,
    imported_tasks: usize,
    imported_task_dispatches: usize,
    imported_reviews: usize,
    imported_review_runs: usize,
    remote_agent_config_imported: bool,
    copied_secret_files: Vec<String>,
    skipped_records: Vec<SkippedLegacyRecord>,
    cleanup_candidates: Vec<CleanupCandidate>,
}
