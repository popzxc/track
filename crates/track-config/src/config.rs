use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use track_types::errors::{ErrorCode, TrackError};
use track_types::types::RemoteAgentPreferredTool;

use crate::paths::{
    collapse_home_path, get_config_path, get_managed_remote_agent_key_path,
    get_managed_remote_agent_known_hosts_path, resolve_path_from_config_file,
};
use crate::runtime::{
    ApiRuntimeConfig, LlamaCppModelSource, LlamaCppRuntimeConfig,
    RemoteAgentReviewFollowUpRuntimeConfig, RemoteAgentRuntimeConfig, TrackRuntimeConfig,
};

pub const DEFAULT_API_PORT: u16 = 3210;
pub const DEFAULT_REMOTE_AGENT_PORT: u16 = 22;
pub const DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT: &str = "~/workspace";
pub const DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH: &str = "~/track-projects.json";
pub const DEFAULT_LLAMACPP_MODEL_HF_REPO: &str = "bartowski/Meta-Llama-3-8B-Instruct-GGUF";
pub const DEFAULT_LLAMACPP_MODEL_HF_FILE: &str = "Meta-Llama-3-8B-Instruct-Q4_K_M.gguf";

// =============================================================================
// Config File Contract
// =============================================================================
//
// The config format stays intentionally small and explicit. Because the
// project is still in active development, we prefer one clear supported shape
// over compatibility branches or implicit migration logic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackConfigFile {
    #[serde(rename = "projectRoots", default)]
    pub project_roots: Vec<String>,
    #[serde(rename = "projectAliases", default)]
    pub project_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub api: ApiConfigFile,
    #[serde(
        rename = "llamaCpp",
        default,
        skip_serializing_if = "LlamaCppConfigFile::is_empty"
    )]
    pub llama_cpp: LlamaCppConfigFile,
    #[serde(
        rename = "remoteAgent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub remote_agent: Option<RemoteAgentConfigFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiConfigFile {
    #[serde(default = "default_api_port")]
    pub port: u16,
}

impl Default for ApiConfigFile {
    fn default() -> Self {
        Self {
            port: default_api_port(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaCppConfigFile {
    #[serde(rename = "modelPath", default, skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
    #[serde(
        rename = "modelHfRepo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub model_hf_repo: Option<String>,
    #[serde(
        rename = "modelHfFile",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub model_hf_file: Option<String>,
}

impl LlamaCppConfigFile {
    fn is_empty(&self) -> bool {
        self.model_path.is_none() && self.model_hf_repo.is_none() && self.model_hf_file.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentConfigFile {
    pub host: String,
    pub user: String,
    #[serde(default = "default_remote_agent_port")]
    pub port: u16,
    #[serde(
        rename = "workspaceRoot",
        default = "default_remote_agent_workspace_root"
    )]
    pub workspace_root: String,
    #[serde(
        rename = "projectsRegistryPath",
        default = "default_remote_projects_registry_path"
    )]
    pub projects_registry_path: String,
    #[serde(
        rename = "preferredTool",
        default,
        skip_serializing_if = "RemoteAgentPreferredTool::is_codex"
    )]
    pub preferred_tool: RemoteAgentPreferredTool,
    #[serde(
        rename = "shellPrelude",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shell_prelude: Option<String>,
    #[serde(
        rename = "reviewFollowUp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAgentReviewFollowUpConfigFile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "mainUser", default, skip_serializing_if = "Option::is_none")]
    pub main_user: Option<String>,
    #[serde(
        rename = "defaultReviewPrompt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_review_prompt: Option<String>,
}

fn default_api_port() -> u16 {
    DEFAULT_API_PORT
}

fn default_remote_agent_port() -> u16 {
    DEFAULT_REMOTE_AGENT_PORT
}

fn default_remote_agent_workspace_root() -> String {
    DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned()
}

fn default_remote_projects_registry_path() -> String {
    DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned()
}

fn default_llama_cpp_model_source() -> LlamaCppModelSource {
    LlamaCppModelSource::HuggingFace {
        repo: DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned(),
        file: DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned(),
    }
}

pub fn canonicalize_optional_multiline_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.replace("\r\n", "\n").trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn canonicalize_remote_agent_config(
    remote_agent: RemoteAgentConfigFile,
) -> Result<RemoteAgentConfigFile, TrackError> {
    let host = remote_agent.host.trim().to_owned();
    let user = remote_agent.user.trim().to_owned();
    let workspace_root = remote_agent.workspace_root.trim().to_owned();
    let projects_registry_path = remote_agent.projects_registry_path.trim().to_owned();
    let shell_prelude = canonicalize_optional_multiline_value(remote_agent.shell_prelude);
    let review_follow_up = remote_agent
        .review_follow_up
        .map(|review_follow_up| {
            let main_user = review_follow_up
                .main_user
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            let default_review_prompt =
                canonicalize_optional_multiline_value(review_follow_up.default_review_prompt);

            if review_follow_up.enabled && main_user.is_none() {
                return Err(TrackError::new(
                    ErrorCode::InvalidRemoteAgentConfig,
                    "Remote review follow-up requires `mainUser` when the feature is enabled.",
                ));
            }

            if !review_follow_up.enabled && main_user.is_none() && default_review_prompt.is_none() {
                return Ok(None);
            }

            Ok(Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: review_follow_up.enabled,
                main_user,
                default_review_prompt,
            }))
        })
        .transpose()?
        .flatten();

    if host.is_empty()
        || user.is_empty()
        || workspace_root.is_empty()
        || projects_registry_path.is_empty()
        || remote_agent.port == 0
    {
        return Err(TrackError::new(
            ErrorCode::InvalidRemoteAgentConfig,
            "Remote agent config requires host, user, workspace root, projects registry path, and a valid SSH port.",
        ));
    }

    Ok(RemoteAgentConfigFile {
        host,
        user,
        port: remote_agent.port,
        workspace_root,
        projects_registry_path,
        preferred_tool: remote_agent.preferred_tool,
        shell_prelude,
        review_follow_up,
    })
}

fn canonicalize_config_file(config: TrackConfigFile) -> Result<TrackConfigFile, TrackError> {
    let project_roots = config
        .project_roots
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let mut project_aliases = BTreeMap::new();
    for (alias, canonical) in config.project_aliases {
        let alias = alias.trim().to_owned();
        let canonical = canonical.trim().to_owned();
        if alias.is_empty() || canonical.is_empty() {
            return Err(TrackError::new(
                ErrorCode::InvalidConfig,
                "Project aliases require both the alias and canonical name to be non-empty.",
            ));
        }
        project_aliases.insert(alias, canonical);
    }

    let model_path = config
        .llama_cpp
        .model_path
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let model_hf_repo = config
        .llama_cpp
        .model_hf_repo
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let model_hf_file = config
        .llama_cpp
        .model_hf_file
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    if model_path.is_some() && (model_hf_repo.is_some() || model_hf_file.is_some()) {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config cannot set both `llamaCpp.modelPath` and the Hugging Face model fields.",
        ));
    }

    if model_hf_repo.is_some() != model_hf_file.is_some() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config requires both `llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile` when using a Hugging Face model.",
        ));
    }

    let remote_agent = config
        .remote_agent
        .map(canonicalize_remote_agent_config)
        .transpose()?;

    Ok(TrackConfigFile {
        project_roots,
        project_aliases,
        api: ApiConfigFile {
            port: config.api.port,
        },
        llama_cpp: LlamaCppConfigFile {
            model_path,
            model_hf_repo,
            model_hf_file,
        },
        remote_agent,
    })
}

pub struct ConfigService {
    config_path: PathBuf,
}

impl ConfigService {
    pub fn new(config_path: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            config_path: match config_path {
                Some(path) => path,
                None => get_config_path()?,
            },
        })
    }

    pub fn resolved_path(&self) -> &Path {
        &self.config_path
    }

    pub fn load_config_file(&self) -> Result<TrackConfigFile, TrackError> {
        let raw_config = fs::read_to_string(&self.config_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return TrackError::new(
                    ErrorCode::ConfigNotFound,
                    format!(
                        "Config file not found at {}",
                        collapse_home_path(&self.config_path)
                    ),
                );
            }

            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not read the track config file: {error}"),
            )
        })?;

        let parsed = serde_json::from_str::<TrackConfigFile>(&raw_config).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Config file is not valid JSON: {error}"),
            )
        })?;

        canonicalize_config_file(parsed)
    }

    pub fn save_config_file(&self, config: &TrackConfigFile) -> Result<(), TrackError> {
        let canonical = canonicalize_config_file(config.clone())?;
        let serialized = serde_json::to_string_pretty(&canonical).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not serialize the track config file: {error}"),
            )
        })?;

        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!(
                        "Could not create the config directory for {}: {error}",
                        collapse_home_path(&self.config_path)
                    ),
                )
            })?;
        }

        fs::write(&self.config_path, format!("{serialized}\n")).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!(
                    "Could not write the track config file at {}: {error}",
                    collapse_home_path(&self.config_path)
                ),
            )
        })
    }

    pub fn load_runtime_config(&self) -> Result<TrackRuntimeConfig, TrackError> {
        let config = self.load_config_file()?;

        // Relative config values should keep working no matter where the user
        // invokes `track` from, so we resolve them relative to the config file
        // itself instead of the caller's current working directory.
        let project_roots = config
            .project_roots
            .iter()
            .map(|value| resolve_path_from_config_file(value, &self.config_path))
            .collect::<Result<Vec<_>, _>>()?;

        let project_aliases = config.project_aliases;
        let model_source = if let (Some(repo), Some(file)) = (
            config.llama_cpp.model_hf_repo.clone(),
            config.llama_cpp.model_hf_file.clone(),
        ) {
            LlamaCppModelSource::HuggingFace { repo, file }
        } else if let Some(model_path) = config.llama_cpp.model_path.as_deref() {
            LlamaCppModelSource::LocalPath(resolve_path_from_config_file(
                model_path,
                &self.config_path,
            )?)
        } else {
            default_llama_cpp_model_source()
        };
        let remote_agent = config
            .remote_agent
            .map(|remote_agent| {
                Ok(RemoteAgentRuntimeConfig {
                    host: remote_agent.host,
                    user: remote_agent.user,
                    port: remote_agent.port,
                    workspace_root: remote_agent.workspace_root,
                    projects_registry_path: remote_agent.projects_registry_path,
                    preferred_tool: remote_agent.preferred_tool,
                    shell_prelude: remote_agent.shell_prelude,
                    review_follow_up: remote_agent.review_follow_up.and_then(|review_follow_up| {
                        review_follow_up.main_user.map(|main_user| {
                            RemoteAgentReviewFollowUpRuntimeConfig {
                                enabled: review_follow_up.enabled,
                                main_user,
                                default_review_prompt: review_follow_up.default_review_prompt,
                            }
                        })
                    }),
                    managed_key_path: get_managed_remote_agent_key_path()?,
                    managed_known_hosts_path: get_managed_remote_agent_known_hosts_path()?,
                })
            })
            .transpose()?;

        Ok(TrackRuntimeConfig {
            project_roots,
            project_aliases,
            api: ApiRuntimeConfig {
                port: config.api.port,
            },
            llama_cpp: LlamaCppRuntimeConfig { model_source },
            remote_agent,
        })
    }

    pub fn load_remote_agent_config(&self) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        Ok(self.load_config_file()?.remote_agent)
    }

    pub fn save_remote_agent_settings(
        &self,
        preferred_tool: RemoteAgentPreferredTool,
        shell_prelude: Option<String>,
        review_follow_up: Option<RemoteAgentReviewFollowUpConfigFile>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let mut config = self.load_config_file()?;
        let Some(remote_agent) = config.remote_agent.as_mut() else {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
            ));
        };

        remote_agent.preferred_tool = preferred_tool;
        remote_agent.shell_prelude = canonicalize_optional_multiline_value(shell_prelude);
        remote_agent.review_follow_up = review_follow_up;
        self.save_config_file(&config)?;

        self.load_config_file()?.remote_agent.ok_or_else(|| {
            TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;
    use track_types::errors::ErrorCode;
    use track_types::types::RemoteAgentPreferredTool;

    use super::{
        default_llama_cpp_model_source, ConfigService, RemoteAgentConfigFile,
        RemoteAgentReviewFollowUpConfigFile, TrackConfigFile, DEFAULT_API_PORT,
        DEFAULT_LLAMACPP_MODEL_HF_FILE, DEFAULT_LLAMACPP_MODEL_HF_REPO, DEFAULT_REMOTE_AGENT_PORT,
        DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT, DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
    };
    use crate::runtime::LlamaCppModelSource;

    fn temp_config_service() -> (TempDir, ConfigService) {
        let directory = TempDir::new().expect("tempdir should be created");
        let config_path = directory.path().join("config.json");
        let service = ConfigService::new(Some(config_path)).expect("config service should resolve");
        (directory, service)
    }

    #[test]
    fn saves_current_local_only_shape() {
        let (_directory, service) = temp_config_service();

        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["~/work".to_owned()],
                project_aliases: BTreeMap::new(),
                api: super::ApiConfigFile {
                    port: DEFAULT_API_PORT,
                },
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: Some("~/.models/parser.gguf".to_owned()),
                    model_hf_repo: None,
                    model_hf_file: None,
                },
                remote_agent: Some(RemoteAgentConfigFile {
                    host: "192.0.2.25".to_owned(),
                    user: "builder".to_owned(),
                    port: DEFAULT_REMOTE_AGENT_PORT,
                    workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
                    projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
                    preferred_tool: RemoteAgentPreferredTool::Codex,
                    shell_prelude: Some("export PATH=\"$PATH:/opt/tools/bin\"".to_owned()),
                    review_follow_up: None,
                }),
            })
            .expect("config should save");

        let saved = fs::read_to_string(service.resolved_path()).expect("config should be readable");
        assert!(saved.contains("\"projectRoots\""));
        assert!(saved.contains("\"llamaCpp\""));
        assert!(saved.contains("\"remoteAgent\""));
    }

    #[test]
    fn loads_default_hugging_face_model_when_no_override_is_saved() {
        let (_directory, service) = temp_config_service();
        service
            .save_config_file(&TrackConfigFile::default())
            .expect("config should save");

        let runtime = service
            .load_runtime_config()
            .expect("runtime config should load");

        assert_eq!(
            runtime.llama_cpp.model_source,
            LlamaCppModelSource::HuggingFace {
                repo: DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned(),
                file: DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned(),
            }
        );
    }

    #[test]
    fn rejects_partial_hugging_face_configuration() {
        let (_directory, service) = temp_config_service();

        let error = service
            .save_config_file(&TrackConfigFile {
                project_roots: Vec::new(),
                project_aliases: BTreeMap::new(),
                api: super::ApiConfigFile::default(),
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: None,
                    model_hf_repo: Some("repo".to_owned()),
                    model_hf_file: None,
                },
                remote_agent: None,
            })
            .expect_err("partial Hugging Face settings should be rejected");

        assert_eq!(error.code, ErrorCode::InvalidConfig);
    }

    #[test]
    fn preserves_explicit_remote_review_follow_up_settings() {
        let (_directory, service) = temp_config_service();
        service
            .save_config_file(&TrackConfigFile {
                project_roots: Vec::new(),
                project_aliases: BTreeMap::new(),
                api: super::ApiConfigFile::default(),
                llama_cpp: super::LlamaCppConfigFile::default(),
                remote_agent: Some(RemoteAgentConfigFile {
                    host: "track.example.com".to_owned(),
                    user: "builder".to_owned(),
                    port: DEFAULT_REMOTE_AGENT_PORT,
                    workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
                    projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
                    preferred_tool: RemoteAgentPreferredTool::Claude,
                    shell_prelude: Some("source ~/.cargo/env".to_owned()),
                    review_follow_up: Some(RemoteAgentReviewFollowUpConfigFile {
                        enabled: true,
                        main_user: Some("popzxc".to_owned()),
                        default_review_prompt: Some("Focus on regressions.".to_owned()),
                    }),
                }),
            })
            .expect("config should save");

        let loaded = service.load_config_file().expect("config should load");
        let review_follow_up = loaded
            .remote_agent
            .expect("remote agent should exist")
            .review_follow_up
            .expect("review follow-up should exist");

        assert!(review_follow_up.enabled);
        assert_eq!(review_follow_up.main_user.as_deref(), Some("popzxc"));
        assert_eq!(
            review_follow_up.default_review_prompt.as_deref(),
            Some("Focus on regressions.")
        );
    }

    #[test]
    fn rejects_enabled_review_follow_up_without_a_main_user() {
        let error = super::canonicalize_remote_agent_config(RemoteAgentConfigFile {
            host: "track.example.com".to_owned(),
            user: "builder".to_owned(),
            port: DEFAULT_REMOTE_AGENT_PORT,
            workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
            projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
            preferred_tool: RemoteAgentPreferredTool::Codex,
            shell_prelude: None,
            review_follow_up: Some(RemoteAgentReviewFollowUpConfigFile {
                enabled: true,
                main_user: None,
                default_review_prompt: None,
            }),
        })
        .expect_err("enabled review follow-up without a main user should fail");

        assert_eq!(error.code, ErrorCode::InvalidRemoteAgentConfig);
    }

    #[test]
    fn default_model_source_matches_saved_default_behavior() {
        assert_eq!(
            default_llama_cpp_model_source(),
            LlamaCppModelSource::HuggingFace {
                repo: DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned(),
                file: DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned(),
            }
        );
    }
}
