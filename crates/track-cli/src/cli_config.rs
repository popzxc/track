use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use track_config::config::{
    LlamaCppConfigFile, DEFAULT_API_PORT, DEFAULT_LLAMACPP_MODEL_HF_FILE,
    DEFAULT_LLAMACPP_MODEL_HF_REPO,
};
use track_config::paths::{collapse_home_path, get_cli_config_path, resolve_path_from_config_file};
use track_config::runtime::{
    ApiRuntimeConfig, LlamaCppModelSource, LlamaCppRuntimeConfig, TrackRuntimeConfig,
};
use track_types::errors::{ErrorCode, TrackError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliConfigFile {
    #[serde(rename = "backendBaseUrl")]
    pub backend_base_url: String,
    #[serde(
        rename = "llamaCpp",
        default,
        skip_serializing_if = "llama_cpp_config_is_empty"
    )]
    pub llama_cpp: LlamaCppConfigFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRuntimeConfig {
    pub backend_base_url: String,
    pub capture_runtime: TrackRuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedCliConfig {
    pub file: CliConfigFile,
    pub runtime: CliRuntimeConfig,
    pub created_default_config: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigureOptions {
    pub backend_base_url: Option<String>,
    pub model_path: Option<String>,
    pub model_hf_repo: Option<String>,
    pub model_hf_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CliConfigService {
    config_path: PathBuf,
}

impl CliConfigService {
    pub fn new(config_path: Option<PathBuf>) -> Result<Self, TrackError> {
        Ok(Self {
            config_path: match config_path {
                Some(path) => path,
                None => get_cli_config_path()?,
            },
        })
    }

    pub fn resolved_path(&self) -> &Path {
        &self.config_path
    }

    pub fn load_or_initialize(&self) -> Result<LoadedCliConfig, TrackError> {
        if self.config_path.exists() {
            return self.load_from_cli_config(false);
        }

        let created = self.save_config_file(&CliConfigFile {
            backend_base_url: default_backend_base_url(),
            llama_cpp: LlamaCppConfigFile::default(),
        })?;

        Ok(LoadedCliConfig {
            runtime: runtime_config_from_file(&created, &self.config_path)?,
            file: created,
            created_default_config: true,
        })
    }

    pub fn configure(&self, options: ConfigureOptions) -> Result<CliConfigFile, TrackError> {
        let existing = self.load_or_initialize()?.file;
        let llama_cpp = if let Some(model_path) = options.model_path {
            LlamaCppConfigFile {
                model_path: Some(model_path),
                model_hf_repo: None,
                model_hf_file: None,
            }
        } else if options.model_hf_repo.is_some() || options.model_hf_file.is_some() {
            LlamaCppConfigFile {
                model_path: None,
                model_hf_repo: options.model_hf_repo,
                model_hf_file: options.model_hf_file,
            }
        } else {
            existing.llama_cpp
        };

        self.save_config_file(&CliConfigFile {
            backend_base_url: options
                .backend_base_url
                .unwrap_or(existing.backend_base_url),
            llama_cpp,
        })
    }

    fn load_from_cli_config(
        &self,
        created_default_config: bool,
    ) -> Result<LoadedCliConfig, TrackError> {
        let file = self.load_config_file()?;

        Ok(LoadedCliConfig {
            runtime: runtime_config_from_file(&file, &self.config_path)?,
            file,
            created_default_config,
        })
    }

    fn load_config_file(&self) -> Result<CliConfigFile, TrackError> {
        let raw_config = fs::read_to_string(&self.config_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                return TrackError::new(
                    ErrorCode::ConfigNotFound,
                    format!(
                        "CLI config file not found at {}.",
                        collapse_home_path(&self.config_path)
                    ),
                );
            }

            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not read the CLI config file: {error}"),
            )
        })?;
        let parsed = serde_json::from_str::<CliConfigFile>(&raw_config).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("CLI config file is not valid JSON: {error}"),
            )
        })?;

        canonicalize_cli_config(parsed)
    }

    fn save_config_file(&self, config: &CliConfigFile) -> Result<CliConfigFile, TrackError> {
        let canonical = canonicalize_cli_config(config.clone())?;
        let serialized = serde_json::to_string_pretty(&canonical).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!("Could not serialize the CLI config file: {error}"),
            )
        })?;

        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!(
                        "Could not create the CLI config directory for {}: {error}",
                        collapse_home_path(&self.config_path)
                    ),
                )
            })?;
        }

        fs::write(&self.config_path, format!("{serialized}\n")).map_err(|error| {
            TrackError::new(
                ErrorCode::InvalidConfig,
                format!(
                    "Could not write the CLI config file at {}: {error}",
                    collapse_home_path(&self.config_path)
                ),
            )
        })?;

        Ok(canonical)
    }
}

fn canonicalize_cli_config(config: CliConfigFile) -> Result<CliConfigFile, TrackError> {
    let backend_base_url = canonicalize_backend_base_url(&config.backend_base_url)?;
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

    if model_hf_repo.is_some() != model_hf_file.is_some() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires both `llamaCpp.modelHfRepo` and `llamaCpp.modelHfFile` when using a Hugging Face model.",
        ));
    }

    Ok(CliConfigFile {
        backend_base_url,
        llama_cpp: LlamaCppConfigFile {
            model_path,
            model_hf_repo,
            model_hf_file,
        },
    })
}

fn canonicalize_backend_base_url(value: &str) -> Result<String, TrackError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires `backendBaseUrl`.",
        ));
    }

    let parsed = url::Url::parse(trimmed).map_err(|error| {
        TrackError::new(
            ErrorCode::InvalidConfig,
            format!("CLI config has an invalid `backendBaseUrl`: {error}"),
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires `backendBaseUrl` to use http or https.",
        ));
    }
    if parsed.host_str().is_none() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires `backendBaseUrl` to include a host name.",
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires `backendBaseUrl` without query or fragment components.",
        ));
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "CLI config requires `backendBaseUrl` to point at the backend origin, not a nested path.",
        ));
    }

    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

fn runtime_config_from_file(
    file: &CliConfigFile,
    config_path: &Path,
) -> Result<CliRuntimeConfig, TrackError> {
    let model_source = if let (Some(repo), Some(file_name)) = (
        file.llama_cpp.model_hf_repo.clone(),
        file.llama_cpp.model_hf_file.clone(),
    ) {
        LlamaCppModelSource::HuggingFace {
            repo,
            file: file_name,
        }
    } else if let Some(model_path) = file.llama_cpp.model_path.as_deref() {
        LlamaCppModelSource::LocalPath(resolve_path_from_config_file(model_path, config_path)?)
    } else {
        LlamaCppModelSource::HuggingFace {
            repo: DEFAULT_LLAMACPP_MODEL_HF_REPO.to_owned(),
            file: DEFAULT_LLAMACPP_MODEL_HF_FILE.to_owned(),
        }
    };

    Ok(CliRuntimeConfig {
        backend_base_url: file.backend_base_url.clone(),
        capture_runtime: TrackRuntimeConfig {
            project_roots: Vec::new(),
            project_aliases: BTreeMap::new(),
            api: ApiRuntimeConfig {
                port: DEFAULT_API_PORT,
            },
            llama_cpp: LlamaCppRuntimeConfig { model_source },
            remote_agent: None,
        },
    })
}

fn default_backend_base_url() -> String {
    "http://127.0.0.1:3210".to_owned()
}

fn llama_cpp_config_is_empty(config: &LlamaCppConfigFile) -> bool {
    config.model_path.is_none() && config.model_hf_repo.is_none() && config.model_hf_file.is_none()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{CliConfigService, ConfigureOptions};

    #[test]
    fn configure_preserves_existing_model_settings_when_only_backend_changes() {
        let directory = TempDir::new().expect("tempdir should be created");
        let cli_config_path = directory.path().join("cli.json");
        let service = CliConfigService::new(Some(cli_config_path))
            .expect("cli config service should resolve");
        service
            .configure(ConfigureOptions {
                model_hf_repo: Some("repo/example".to_owned()),
                model_hf_file: Some("model.gguf".to_owned()),
                ..ConfigureOptions::default()
            })
            .expect("initial config should save");

        let configured = service
            .configure(ConfigureOptions {
                backend_base_url: Some("http://127.0.0.1:33210".to_owned()),
                ..ConfigureOptions::default()
            })
            .expect("backend url update should preserve model settings");

        assert_eq!(configured.backend_base_url, "http://127.0.0.1:33210");
        assert_eq!(
            configured.llama_cpp.model_hf_repo.as_deref(),
            Some("repo/example")
        );
        assert_eq!(
            configured.llama_cpp.model_hf_file.as_deref(),
            Some("model.gguf")
        );
    }
}
