use serde::{Deserialize, Serialize};

pub const MIGRATION_STATUS_SETTING_KEY: &str = "migration_status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    Ready,
    ImportRequired,
    Imported,
    // The skip flow was removed, but we still accept the legacy serialized
    // value so older backend_settings rows do not become unreadable.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyScanSummary {
    #[serde(rename = "projectsFound")]
    pub projects_found: usize,
    #[serde(rename = "aliasesFound")]
    pub aliases_found: usize,
    #[serde(rename = "tasksFound")]
    pub tasks_found: usize,
    #[serde(rename = "taskDispatchesFound")]
    pub task_dispatches_found: usize,
    #[serde(rename = "reviewsFound")]
    pub reviews_found: usize,
    #[serde(rename = "reviewRunsFound")]
    pub review_runs_found: usize,
    #[serde(rename = "remoteAgentConfigured")]
    pub remote_agent_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedLegacyRecord {
    pub kind: String,
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupCandidate {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationStatus {
    pub state: MigrationState,
    #[serde(rename = "requiresMigration")]
    pub requires_migration: bool,
    #[serde(rename = "canImport")]
    pub can_import: bool,
    #[serde(rename = "legacyDetected")]
    pub legacy_detected: bool,
    pub summary: LegacyScanSummary,
    #[serde(rename = "skippedRecords", default)]
    pub skipped_records: Vec<SkippedLegacyRecord>,
    #[serde(rename = "cleanupCandidates", default)]
    pub cleanup_candidates: Vec<CleanupCandidate>,
}

impl MigrationStatus {
    pub fn ready() -> Self {
        Self {
            state: MigrationState::Ready,
            requires_migration: false,
            can_import: false,
            legacy_detected: false,
            summary: LegacyScanSummary::default(),
            skipped_records: Vec::new(),
            cleanup_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MigrationImportSummary {
    #[serde(rename = "importedProjects")]
    pub imported_projects: usize,
    #[serde(rename = "importedAliases")]
    pub imported_aliases: usize,
    #[serde(rename = "importedTasks")]
    pub imported_tasks: usize,
    #[serde(rename = "importedTaskDispatches")]
    pub imported_task_dispatches: usize,
    #[serde(rename = "importedReviews")]
    pub imported_reviews: usize,
    #[serde(rename = "importedReviewRuns")]
    pub imported_review_runs: usize,
    #[serde(rename = "remoteAgentConfigImported")]
    pub remote_agent_config_imported: bool,
    #[serde(rename = "copiedSecretFiles", default)]
    pub copied_secret_files: Vec<String>,
    #[serde(rename = "skippedRecords", default)]
    pub skipped_records: Vec<SkippedLegacyRecord>,
    #[serde(rename = "cleanupCandidates", default)]
    pub cleanup_candidates: Vec<CleanupCandidate>,
}
