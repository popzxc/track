use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, TrackError};
use crate::paths::{
    collapse_home_path, get_config_path, resolve_optional_command_path_from_config_file,
    resolve_path_from_config_file,
};
use crate::types::{LlamaCppRuntimeConfig, TrackRuntimeConfig};

// =============================================================================
// Config File Contract
// =============================================================================
//
// The config format is intentionally small and explicit. Because the project is
// still in active development, we prefer one clear supported shape over a pile
// of upgrade-era compatibility branches.
//
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackConfigFile {
    #[serde(rename = "projectRoots", default)]
    pub project_roots: Vec<String>,
    #[serde(rename = "projectAliases", default)]
    pub project_aliases: BTreeMap<String, String>,
    #[serde(rename = "llamaCpp")]
    pub llama_cpp: LlamaCppConfigFile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaCppConfigFile {
    #[serde(rename = "modelPath")]
    pub model_path: String,
    #[serde(
        rename = "llamaCompletionPath",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub llama_completion_path: Option<String>,
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

    let model_path = config.llama_cpp.model_path.trim().to_owned();
    if model_path.is_empty() {
        return Err(TrackError::new(
            ErrorCode::InvalidConfig,
            "Config file does not match the expected format.",
        ));
    }

    let llama_completion_path = config
        .llama_cpp
        .llama_completion_path
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    Ok(TrackConfigFile {
        project_roots,
        project_aliases,
        llama_cpp: LlamaCppConfigFile {
            model_path,
            llama_completion_path,
        },
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

        let model_path =
            resolve_path_from_config_file(&config.llama_cpp.model_path, &self.config_path)?;
        let llama_completion_path = resolve_optional_command_path_from_config_file(
            config.llama_cpp.llama_completion_path.as_deref(),
            &self.config_path,
        )?;

        Ok(TrackRuntimeConfig {
            project_roots,
            project_aliases,
            llama_cpp: LlamaCppRuntimeConfig {
                model_path,
                llama_completion_path,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    use super::{ConfigService, TrackConfigFile};

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
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: "~/.models/parser.gguf".to_owned(),
                    llama_completion_path: None,
                },
            })
            .expect("config should save");

        let raw =
            fs::read_to_string(service.resolved_path()).expect("saved config should be readable");
        assert!(raw.contains("\"llamaCpp\""));
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
                llama_cpp: super::LlamaCppConfigFile {
                    model_path: "./models/parser.gguf".to_owned(),
                    llama_completion_path: Some("../bin/llama-completion".to_owned()),
                },
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
        assert_eq!(
            runtime.llama_cpp.model_path,
            config_directory.join("./models/parser.gguf")
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
    }
}
