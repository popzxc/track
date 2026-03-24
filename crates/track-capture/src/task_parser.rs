use std::env;
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

use crate::llama_cpp::LlamaCppTaskParser;
use crate::llama_cpp_2::LlamaCpp2TaskParser;

pub const TRACK_TASK_PARSER_ENV_VAR: &str = "TRACK_TASK_PARSER";
const DEFAULT_TASK_PARSER_BACKEND: &str = "llama-completion";
const LLAMA_CPP_2_TASK_PARSER_BACKEND: &str = "llama-cpp-2";

// =============================================================================
// Task Parser Selection
// =============================================================================
//
// Capture should not need to know which local inference backend is active or
// whether the configured model comes from a stable local file or a managed
// Hugging Face cache. Centralizing that decision here keeps the rest of the
// capture flow focused on task validation and persistence.

pub trait TaskParser {
    fn parse_task(
        &self,
        raw_text: &str,
        project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskParserBackend {
    LlamaCompletion,
    LlamaCpp2,
}

pub fn create_task_parser(
    config: &TrackRuntimeConfig,
) -> Result<Box<dyn TaskParser + 'static>, TrackError> {
    let backend = selected_task_parser_backend()?;
    let model_path = resolve_model_path(&config.llama_cpp)?;

    Ok(match backend {
        TaskParserBackend::LlamaCompletion => Box::new(LlamaCppTaskParser::new(
            config.llama_cpp.llama_completion_path.clone(),
            model_path,
        )),
        TaskParserBackend::LlamaCpp2 => Box::new(LlamaCpp2TaskParser::new(model_path)),
    })
}

fn selected_task_parser_backend() -> Result<TaskParserBackend, TrackError> {
    let Some(value) = env::var(TRACK_TASK_PARSER_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(TaskParserBackend::LlamaCompletion);
    };

    match value.as_str() {
        DEFAULT_TASK_PARSER_BACKEND => Ok(TaskParserBackend::LlamaCompletion),
        LLAMA_CPP_2_TASK_PARSER_BACKEND => Ok(TaskParserBackend::LlamaCpp2),
        _ => Err(TrackError::new(
            ErrorCode::InvalidConfigInput,
            format!(
                "Unsupported {TRACK_TASK_PARSER_ENV_VAR} value `{value}`. Expected `{DEFAULT_TASK_PARSER_BACKEND}` or `{LLAMA_CPP_2_TASK_PARSER_BACKEND}`."
            ),
        )),
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    use super::{selected_task_parser_backend, TaskParserBackend, TRACK_TASK_PARSER_ENV_VAR};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        previous_value: Option<OsString>,
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous_value.take() {
                Some(previous_value) => std::env::set_var(TRACK_TASK_PARSER_ENV_VAR, previous_value),
                None => std::env::remove_var(TRACK_TASK_PARSER_ENV_VAR),
            }
        }
    }

    fn set_task_parser_env(value: Option<&str>) -> EnvVarGuard {
        let previous_value = std::env::var_os(TRACK_TASK_PARSER_ENV_VAR);
        match value {
            Some(value) => std::env::set_var(TRACK_TASK_PARSER_ENV_VAR, value),
            None => std::env::remove_var(TRACK_TASK_PARSER_ENV_VAR),
        }

        EnvVarGuard { previous_value }
    }

    #[test]
    fn defaults_to_legacy_llama_completion_backend() {
        let _lock = env_lock().lock().expect("env lock should be available");
        let _guard = set_task_parser_env(None);

        let backend =
            selected_task_parser_backend().expect("default parser selection should succeed");

        assert_eq!(backend, TaskParserBackend::LlamaCompletion);
    }

    #[test]
    fn selects_llama_cpp_2_backend_from_env() {
        let _lock = env_lock().lock().expect("env lock should be available");
        let _guard = set_task_parser_env(Some("llama-cpp-2"));

        let backend =
            selected_task_parser_backend().expect("llama-cpp-2 parser selection should succeed");

        assert_eq!(backend, TaskParserBackend::LlamaCpp2);
    }

    #[test]
    fn rejects_unknown_task_parser_backend() {
        let _lock = env_lock().lock().expect("env lock should be available");
        let _guard = set_task_parser_env(Some("mystery-backend"));

        let error =
            selected_task_parser_backend().expect_err("unknown parser selection should fail");

        assert!(error
            .message()
            .contains("Unsupported TRACK_TASK_PARSER value `mystery-backend`"));
    }
}
