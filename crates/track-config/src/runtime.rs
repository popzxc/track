use std::collections::BTreeMap;
use std::path::PathBuf;

use track_types::types::RemoteAgentPreferredTool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlamaCppModelSource {
    LocalPath(PathBuf),
    HuggingFace { repo: String, file: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaCppRuntimeConfig {
    pub model_source: LlamaCppModelSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRuntimeConfig {
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentReviewFollowUpRuntimeConfig {
    pub enabled: bool,
    pub main_user: String,
    pub default_review_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAgentRuntimeConfig {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub workspace_root: String,
    pub projects_registry_path: String,
    pub preferred_tool: RemoteAgentPreferredTool,
    pub shell_prelude: Option<String>,
    pub review_follow_up: Option<RemoteAgentReviewFollowUpRuntimeConfig>,
    pub managed_key_path: PathBuf,
    pub managed_known_hosts_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRuntimeConfig {
    pub project_roots: Vec<PathBuf>,
    pub project_aliases: BTreeMap<String, String>,
    pub api: ApiRuntimeConfig,
    pub llama_cpp: LlamaCppRuntimeConfig,
    pub remote_agent: Option<RemoteAgentRuntimeConfig>,
}
