use std::fs;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use hf_hub::api::sync::ApiBuilder;
use hf_hub::Cache;
use serde_json::from_str;
use track_core::errors::{ErrorCode, TrackError};
use track_core::paths::{collapse_home_path, get_models_dir};
use track_core::project_catalog::ProjectCatalog;
use track_core::types::{
    LlamaCppModelSource, LlamaCppRuntimeConfig, ParsedTaskCandidate, TrackRuntimeConfig,
};

use crate::llama_cpp_2::LlamaCpp2TaskParser;

const TRACK_TEST_INFERENCE_ENV: &str = "TRACK_TEST_INFERENCE";
const TRACK_TEST_INFERENCE_RESULT_ENV: &str = "TRACK_TEST_INFERENCE_RESULT";

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
        // The smoke suite needs one deterministic, model-free capture path so
        // it can validate the installed CLI without downloading a model or
        // depending on inference variability. This stays intentionally hidden
        // behind an env var and raw JSON input so user-facing docs do not
        // accidentally turn an internal test seam into a supported workflow.
        if test_inference_enabled() {
            return Ok(Box::new(TestInferenceTaskParser));
        }

        let model_path = resolve_model_path(&config.llama_cpp)?;
        Ok(Box::new(LlamaCpp2TaskParser::new(model_path)))
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct TestInferenceTaskParser;

impl TaskParser for TestInferenceTaskParser {
    fn parse_task(
        &self,
        raw_text: &str,
        _project_catalog: &ProjectCatalog,
    ) -> Result<ParsedTaskCandidate, TrackError> {
        let candidate_json = std::env::var(TRACK_TEST_INFERENCE_RESULT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| raw_text.to_owned());

        from_str::<ParsedTaskCandidate>(&candidate_json).map_err(|error| {
            TrackError::new(
                ErrorCode::AiParseFailed,
                format!(
                    "AI parse failure. `{TRACK_TEST_INFERENCE_ENV}` expects valid ParsedTaskCandidate JSON in either `{TRACK_TEST_INFERENCE_RESULT_ENV}` or the capture text: {error}"
                ),
            )
        })
    }
}

fn test_inference_enabled() -> bool {
    let Ok(value) = std::env::var(TRACK_TEST_INFERENCE_ENV) else {
        return false;
    };

    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    !matches!(normalized.as_str(), "0" | "false" | "no" | "off")
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
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use serde_json::json;
    use track_core::types::{
        ApiRuntimeConfig, Confidence, LlamaCppModelSource, LlamaCppRuntimeConfig, Priority,
        TrackRuntimeConfig,
    };

    use super::{
        test_inference_enabled, LocalTaskParserFactory, TaskParserFactory,
        TRACK_TEST_INFERENCE_ENV, TRACK_TEST_INFERENCE_RESULT_ENV,
    };

    static TEST_INFERENCE_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn runtime_config() -> TrackRuntimeConfig {
        TrackRuntimeConfig {
            project_roots: Vec::new(),
            project_aliases: BTreeMap::new(),
            api: ApiRuntimeConfig { port: 3210 },
            llama_cpp: LlamaCppRuntimeConfig {
                model_source: LlamaCppModelSource::LocalPath(PathBuf::from(
                    "/definitely/not/a/real/model.gguf",
                )),
            },
            remote_agent: None,
        }
    }

    #[test]
    fn test_inference_parser_bypasses_model_resolution() {
        let _guard = TEST_INFERENCE_ENV_LOCK
            .lock()
            .expect("test inference env mutex should not be poisoned");
        std::env::set_var(TRACK_TEST_INFERENCE_ENV, "1");

        let parser = LocalTaskParserFactory
            .create_parser(&runtime_config())
            .expect("test inference parser should bypass model resolution");
        let test_input = json!({
            "project": "project-a",
            "priority": "high",
            "title": "Smoke task",
            "bodyMarkdown": "- Verify the installed CLI path.",
            "confidence": "high",
        })
        .to_string();
        let parsed = parser
            .parse_task(
                &test_input,
                &track_core::project_catalog::ProjectCatalog::new(Vec::new()),
            )
            .expect("test inference JSON should parse");
        assert_eq!(parsed.project.as_deref(), Some("project-a"));
        assert_eq!(parsed.priority, Priority::High);
        assert_eq!(parsed.confidence, Confidence::High);

        std::env::remove_var(TRACK_TEST_INFERENCE_ENV);
    }

    #[test]
    fn test_inference_parser_reads_result_from_env_var() {
        let _guard = TEST_INFERENCE_ENV_LOCK
            .lock()
            .expect("test inference env mutex should not be poisoned");
        std::env::set_var(TRACK_TEST_INFERENCE_ENV, "1");
        std::env::set_var(
            TRACK_TEST_INFERENCE_RESULT_ENV,
            json!({
                "project": "project-a",
                "priority": "high",
                "title": "Smoke task from env",
                "bodyMarkdown": "- Verify the installed CLI path.",
                "confidence": "high",
            })
            .to_string(),
        );

        let parser = LocalTaskParserFactory
            .create_parser(&runtime_config())
            .expect("test inference parser should bypass model resolution");
        let parsed = parser
            .parse_task(
                "project-a prio high verify the installed CLI path",
                &track_core::project_catalog::ProjectCatalog::new(Vec::new()),
            )
            .expect("test inference env JSON should parse");
        assert_eq!(parsed.project.as_deref(), Some("project-a"));
        assert_eq!(parsed.title, "Smoke task from env");
        assert_eq!(parsed.priority, Priority::High);

        std::env::remove_var(TRACK_TEST_INFERENCE_RESULT_ENV);
        std::env::remove_var(TRACK_TEST_INFERENCE_ENV);
    }

    #[test]
    fn test_inference_truthy_and_falsey_values_are_recognized() {
        let _guard = TEST_INFERENCE_ENV_LOCK
            .lock()
            .expect("test inference env mutex should not be poisoned");

        for truthy in ["1", "true", "yes", "on"] {
            std::env::set_var(TRACK_TEST_INFERENCE_ENV, truthy);
            assert!(
                test_inference_enabled(),
                "{truthy} should enable test inference"
            );
        }

        for falsey in ["", "0", "false", "no", "off"] {
            std::env::set_var(TRACK_TEST_INFERENCE_ENV, falsey);
            assert!(
                !test_inference_enabled(),
                "{falsey:?} should disable test inference"
            );
        }

        std::env::remove_var(TRACK_TEST_INFERENCE_ENV);
        std::env::remove_var(TRACK_TEST_INFERENCE_RESULT_ENV);
    }
}
