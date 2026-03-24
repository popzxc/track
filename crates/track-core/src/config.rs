use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, TrackError};
use crate::paths::{
    collapse_home_path, get_config_path, get_managed_remote_agent_key_path,
    get_managed_remote_agent_known_hosts_path, resolve_optional_command_path_from_config_file,
    resolve_path_from_config_file,
};
use crate::types::{
    ApiRuntimeConfig, LlamaCppModelSource, LlamaCppRuntimeConfig, RemoteAgentRuntimeConfig,
    TrackRuntimeConfig,
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
    #[serde(rename = "llamaCpp")]
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
    #[serde(rename = "modelHfRepo", default, skip_serializing_if = "Option::is_none")]
    pub model_hf_repo: Option<String>,
    #[serde(rename = "modelHfFile", default, skip_serializing_if = "Option::is_none")]
    pub model_hf_file: Option<String>,
    #[serde(
        rename = "llamaCompletionPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub llama_completion_path: Option<String>,
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
        rename = "shellPrelude",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shell_prelude: Option<String>,
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

fn canonicalize_optional_multiline_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.replace("\r\n", "\n").trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn canonicalize_config_file(config: TrackConfigFile) -> Result<TrackConfigFile, TrackError> {
    let project_roots = config
        .project_roots
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let project_aliases = config
        .project_aliases
        .into_iter()
        .map(|(alias, canonical_name)| (alias.trim().to_owned(), canonical_name.trim().to_owned()))
        .filter(|(alias, canonical_name)| !alias.is_empty() && !canonical_name.is_empty())
        .collect::<BTreeMap<_, _>>();

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
    let has_hf_model = model_hf_repo.is_some() || model_hf_file.is_some();
    if model_path.is_none() && !has_hf_model {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config file does not match the expected format.",
        ));
    }
    if model_hf_repo.is_some() != model_hf_file.is_some() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config file requires both `llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile` when using a Hugging Face model.",
        ));
    }

    let llama_completion_path = config
        .llama_cpp
        .llama_completion_path
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let api_port = config.api.port;
    if api_port == 0 {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config file does not match the expected format.",
        ));
    }

    let remote_agent = config
        .remote_agent
        .map(|remote_agent| {
            let host = remote_agent.host.trim().to_owned();
            let user = remote_agent.user.trim().to_owned();
            let workspace_root = remote_agent.workspace_root.trim().to_owned();
            let projects_registry_path = remote_agent.projects_registry_path.trim().to_owned();
            let shell_prelude = canonicalize_optional_multiline_value(remote_agent.shell_prelude);

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
                shell_prelude,
            })
        })
        .transpose()?;

    Ok(TrackConfigFile {
        project_roots,
        project_aliases,
        api: ApiConfigFile { port: api_port },
        llama_cpp: LlamaCppConfigFile {
            model_path,
            model_hf_repo,
            model_hf_file,
            llama_completion_path,
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
        } else {
            let model_path = config
                .llama_cpp
                .model_path
                .as_deref()
                .ok_or_else(|| {
                    TrackError::new(
                        ErrorCode::InvalidConfig,
                        "Config file does not match the expected format.",
                    )
                })
                .and_then(|value| resolve_path_from_config_file(value, &self.config_path))?;
            LlamaCppModelSource::LocalPath(model_path)
        };
        let llama_completion_path = resolve_optional_command_path_from_config_file(
            config.llama_cpp.llama_completion_path.as_deref(),
            &self.config_path,
        )?;
        let remote_agent = config
            .remote_agent
            .map(|remote_agent| {
                Ok(RemoteAgentRuntimeConfig {
                    host: remote_agent.host,
                    user: remote_agent.user,
                    port: remote_agent.port,
                    workspace_root: remote_agent.workspace_root,
                    projects_registry_path: remote_agent.projects_registry_path,
                    shell_prelude: remote_agent.shell_prelude,
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
            llama_cpp: LlamaCppRuntimeConfig {
                model_source,
                llama_completion_path,
            },
            remote_agent,
        })
    }

    pub fn load_remote_agent_config(&self) -> Result<Option<RemoteAgentConfigFile>, TrackError> {
        Ok(self.load_config_file()?.remote_agent)
    }

    pub fn save_remote_agent_shell_prelude(
        &self,
        shell_prelude: Option<String>,
    ) -> Result<RemoteAgentConfigFile, TrackError> {
        let mut config = self.load_config_file()?;
        let Some(remote_agent) = config.remote_agent.as_mut() else {
            return Err(TrackError::new(
                ErrorCode::RemoteAgentNotConfigured,
                "Remote dispatch is not configured yet. Re-run `track` and add a remote agent host plus SSH key.",
            ));
        };

        remote_agent.shell_prelude = canonicalize_optional_multiline_value(shell_prelude);
        self.save_config_file(&config)?;

        self.load_config_file()?
            .remote_agent
            .ok_or_else(|| {
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

    use super::{
        ConfigService, RemoteAgentConfigFile, TrackConfigFile, DEFAULT_API_PORT,
        DEFAULT_LLAMACPP_MODEL_HF_FILE, DEFAULT_LLAMACPP_MODEL_HF_REPO,
        DEFAULT_REMOTE_AGENT_PORT, DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT,
        DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH,
    };
    use crate::types::LlamaCppModelSource;

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
                    llama_completion_path: None,
                },
                remote_agent: Some(RemoteAgentConfigFile {
                    host: "192.0.2.25".to_owned(),
                    user: "builder".to_owned(),
                    port: DEFAULT_REMOTE_AGENT_PORT,
                    workspace_root: DEFAULT_REMOTE_AGENT_WORKSPACE_ROOT.to_owned(),
                    projects_registry_path: DEFAULT_REMOTE_PROJECTS_REGISTRY_PATH.to_owned(),
                    shell_prelude: Some("export PATH=\"$PATH:/opt/tools/bin\"".to_owned()),
                }),
            })
            .expect("config should save");

        let raw =
            fs::read_to_string(service.resolved_path()).expect("saved config should be readable");
        assert!(raw.contains("\"llamaCpp\""));
        assert!(raw.contains("\"remoteAgent\""));
        assert!(raw.contains("\"shellPrelude\""));
        assert!(!raw.contains("\"modelHfRepo\""));
        assert!(!raw.contains("\"ai\""));
    }

    #[test]
    fn resolves_relative_runtime_paths_from_the_config_file_location() {
        let directory = TempDir::new().expect("tempdir should be created");
        let config_path = directory.path().join(".config/track/config.json");
        let service =
            ConfigService::new(Some(config_path.clone())).expect("config service should resolve");

        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["../work".to_owned()],
                project_aliases: BTreeMap::new(),
                api: super::ApiConfigFile { port: 4210 },
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: Some("./models/parser.gguf".to_owned()),
                    model_hf_repo: None,
                    model_hf_file: None,
                    llama_completion_path: Some("../bin/llama-completion".to_owned()),
                },
                remote_agent: Some(RemoteAgentConfigFile {
                    host: "192.0.2.25".to_owned(),
                    user: "builder".to_owned(),
                    port: 2222,
                    workspace_root: "~/workspace".to_owned(),
                    projects_registry_path: "~/track-projects.json".to_owned(),
                    shell_prelude: Some("export PATH=\"$PATH:/opt/tools/bin\"".to_owned()),
                }),
            })
            .expect("config should save");

        let runtime = service
            .load_runtime_config()
            .expect("runtime config should resolve");
        let config_directory = config_path
            .parent()
            .expect("config path should have a parent");

        assert_eq!(
            runtime.project_roots,
            vec![config_directory.join("../work")]
        );
        assert_eq!(runtime.api.port, 4210);
        assert_eq!(
            runtime.llama_cpp.model_source,
            LlamaCppModelSource::LocalPath(config_directory.join("./models/parser.gguf"))
        );
        assert_eq!(
            runtime.llama_cpp.llama_completion_path,
            Some(
                config_directory
                    .join("../bin/llama-completion")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        let remote_agent = runtime
            .remote_agent
            .expect("remote agent runtime config should resolve");
        assert_eq!(remote_agent.host, "192.0.2.25");
        assert_eq!(remote_agent.user, "builder");
        assert_eq!(remote_agent.port, 2222);
        assert_eq!(
            remote_agent.shell_prelude,
            Some("export PATH=\"$PATH:/opt/tools/bin\"".to_owned())
        );
    }

    #[test]
    fn prefers_hugging_face_model_when_both_sources_are_configured() {
        let (_directory, service) = temp_config_service();

        service
            .save_config_file(&TrackConfigFile {
                project_roots: vec!["~/work".to_owned()],
                project_aliases: BTreeMap::new(),
                api: super::ApiConfigFile {
                    port: DEFAULT_API_PORT,
                },
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: Some("~/.models/legacy-parser.gguf".to_owned()),
                    model_hf_repo: Some(DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned()),
                    model_hf_file: Some(DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned()),
                    llama_completion_path: None,
                },
                remote_agent: None,
            })
            .expect("config should save");

        let runtime = service
            .load_runtime_config()
            .expect("runtime config should resolve");

        assert_eq!(
            runtime.llama_cpp.model_source,
            LlamaCppModelSource::HuggingFace {
                repo: DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned(),
                file: DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned(),
            }
        );
    }
}
