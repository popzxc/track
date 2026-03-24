use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use hf_hub::api::sync::ApiBuilder;
use hf_hub::Cache;
use track_core::errors::{ErrorCode, TrackError};
use track_core::paths::{collapse_home_path, get_models_dir};
use track_core::project_catalog::ProjectCatalog;
use track_core::types::{
    LlamaCppModelSource, LlamaCppRuntimeConfig, ParsedTaskCandidate, TrackRuntimeConfig,
};

use crate::llama_cpp_2::LlamaCpp2TaskParser;

// =============================================================================
// Task Parser Construction
// =============================================================================
//
// Capture should not need to know whether the configured model comes from a
// stable local file or a managed Hugging Face cache. Centralizing that
// decision here keeps the rest of the capture flow focused on task validation
// and persistence.

pub trait TaskParser {
    fn parse_task(
        &self,
        raw_text: &str,
        project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError>;
}

pub trait TaskParserFactory {
    fn create_parser(
        &self,
        config: &TrackRuntimeConfig,
    ) -> Result<Box<dyn TaskParser + 'static>, TrackError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalTaskParserFactory;

impl TaskParserFactory for LocalTaskParserFactory {
    fn create_parser(
        &self,
        config: &TrackRuntimeConfig,
    ) -> Result<Box<dyn TaskParser + 'static>, TrackError> {
        let model_path = resolve_model_path(&config.llama_cpp)?;
        Ok(Box::new(LlamaCpp2TaskParser::new(model_path)))
    }
}

pub fn resolve_model_path(config: &LlamaCppRuntimeConfig) -> Result<PathBuf, TrackError> {
    match &config.model_source {
        LlamaCppModelSource::LocalPath(path) => Ok(path.clone()),
        LlamaCppModelSource::HuggingFace { repo, file } => {
            let models_dir = get_models_dir()?;
            fs::create_dir_all(&models_dir).map_err(|error| {
                TrackError::new(
                    ErrorCode::InvalidConfig,
                    format!(
                        "Could not create the local model cache directory at {}: {error}",
                        collapse_home_path(&models_dir)
                    ),
                )
            })?;

            let cache = Cache::new(models_dir.clone());
            if let Some(path) = cache.model(repo.clone()).get(file) {
                return Ok(path);
            }

            let show_progress = io::stderr().is_terminal();
            eprintln!(
                "Downloading model {repo}/{file} into {}. This can take a while on first use.",
                collapse_home_path(&models_dir)
            );

            // We start from `from_env()` so HF auth and endpoint overrides keep
            // working as users expect. We then override only the cache root so
            // downloaded GGUF assets live under `~/.track/models`.
            let api = ApiBuilder::from_env()
                .with_cache_dir(models_dir.clone())
                .with_progress(show_progress)
                .with_retries(2)
                .with_user_agent("track", env!("CARGO_PKG_VERSION"))
                .build()
                .map_err(|error| {
                    TrackError::new(
                        ErrorCode::AiParseFailed,
                        format!("AI parse failure. Could not initialize the Hugging Face client: {error}"),
                    )
                })?;

            api.model(repo.clone()).download(file).map_err(|error| {
                TrackError::new(
                    ErrorCode::AiParseFailed,
                    format!(
                        "AI parse failure. Could not download the configured Hugging Face model `{repo}` / `{file}` into {}: {error}",
                        collapse_home_path(&models_dir)
                    ),
                )
            })
        }
    }
}
